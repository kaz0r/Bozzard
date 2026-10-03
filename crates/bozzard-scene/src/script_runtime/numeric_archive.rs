//! Bounded numeric archive packing and row gathering for replay and scene hooks.
//!
//! Native array operations keep dense saved grids from spending the interpreter's
//! loop budget on serialization. The wire format remains comma-separated integer
//! values, with `value:length` for runs of three or more identical values.
use super::{Array, Dynamic, Engine, EvalAltResult, ImmutableString, MAX_SCRIPT_BYTES, fail};

const MAX_VALUES: usize = 1 << 16;
type Result<T> = std::result::Result<T, Box<EvalAltResult>>;

fn integer(value: &Dynamic) -> Result<rhai::INT> {
    if let Some(value) = value.clone().try_cast::<rhai::INT>() {
        return Ok(value);
    }
    match value.clone().try_cast::<rhai::FLOAT>() {
        Some(value) if value.is_finite() => Ok(value as rhai::INT),
        _ => Err(fail("numeric archive requires finite numbers")),
    }
}

fn indices(cells: &Array, stride: rhai::INT) -> Result<(Vec<usize>, usize)> {
    let stride = usize::try_from(stride)
        .ok()
        .filter(|n| (1..=MAX_VALUES).contains(n))
        .ok_or_else(|| fail("numeric archive stride must be between 1 and 65536"))?;
    if cells.len() > MAX_VALUES / stride {
        return Err(fail("numeric archive exceeds 65536 values"));
    }
    let mut result = Vec::with_capacity(cells.len());
    for cell in cells {
        let cell = integer(cell)?;
        let cell = usize::try_from(cell)
            .ok()
            .filter(|&cell| cell < MAX_VALUES / stride)
            .ok_or_else(|| fail("numeric archive cell is out of bounds"))?;
        if result.last().is_some_and(|&previous| cell <= previous) {
            return Err(fail("numeric archive cells must be sorted and unique"));
        }
        result.push(cell);
    }
    Ok((result, stride))
}

#[derive(Default)]
struct Encoder {
    text: String,
    value: rhai::INT,
    length: usize,
}
impl Encoder {
    fn append(&mut self, value: rhai::INT, length: usize) {
        if length == 0 {
            return;
        }
        if self.length != 0 && self.value != value {
            self.flush();
        }
        self.value = value;
        self.length += length;
    }
    fn flush(&mut self) {
        use std::fmt::Write;
        if self.length == 0 {
            return;
        }
        if !self.text.is_empty() {
            self.text.push(',');
        }
        if self.length >= 3 {
            write!(self.text, "{}:{}", self.value, self.length).unwrap();
        } else {
            write!(self.text, "{}", self.value).unwrap();
            if self.length == 2 {
                write!(self.text, ",{}", self.value).unwrap();
            }
        }
        self.length = 0;
    }
}

fn pack(values: Array, cells: Array, stride: rhai::INT) -> Result<ImmutableString> {
    if values.len() > MAX_VALUES {
        return Err(fail("numeric archive exceeds 65536 values"));
    }
    let (cells, stride) = indices(&cells, stride)?;
    if cells.is_empty() {
        return Ok("".into());
    }
    let mut encoder = Encoder::default();
    let mut next = 0;
    for cell in cells {
        let start = cell * stride;
        let end = start + stride;
        if end > values.len() {
            return Err(fail("numeric archive cell is outside the value array"));
        }
        encoder.append(0, start - next);
        for value in &values[start..end] {
            encoder.append(integer(value)?, 1);
        }
        next = end;
    }
    encoder.append(0, values.len() - next);
    encoder.flush();
    if encoder.text.len() > MAX_SCRIPT_BYTES {
        return Err(fail("numeric archive exceeds the script string limit"));
    }
    Ok(encoder.text.into())
}

fn unpack(text: ImmutableString, cells: Array, stride: rhai::INT) -> Result<Array> {
    if text.len() > MAX_SCRIPT_BYTES {
        return Err(fail("numeric archive exceeds the script string limit"));
    }
    let (cells, stride) = indices(&cells, stride)?;
    let count = cells.len() * stride;
    if count == 0 || text.is_empty() {
        return Ok(vec![Dynamic::from_float(0.0); count]);
    }
    let mut values = Vec::with_capacity(count);
    let mut cursor = 0usize;
    for part in text.split(',') {
        let (value, length) = part.split_once(':').unwrap_or((part, "1"));
        let value = value
            .parse::<rhai::INT>()
            .map_err(|_| fail("numeric archive has an invalid integer"))?;
        let length = length
            .parse::<usize>()
            .ok()
            .filter(|&n| n > 0 && n <= MAX_VALUES - cursor)
            .ok_or_else(|| fail("numeric archive has an invalid run length"))?;
        let end = cursor + length;
        while values.len() < count
            && cells[values.len() / stride] * stride + values.len() % stride < end
        {
            values.push(Dynamic::from_float(value as rhai::FLOAT));
        }
        cursor = end;
    }
    if values.len() == count {
        Ok(values)
    } else {
        Err(fail("numeric archive does not cover the requested cells"))
    }
}

/// Gather paired columns into one row per selected cell, preserving page/slot
/// order and numeric types. The interpreter otherwise clones/indexes large
/// pages repeatedly while assembling the same small rows.
fn gather_pairs(kinds: Array, amounts: Array, cells: Array, stride: rhai::INT) -> Result<Array> {
    let (cells, stride) = indices(&cells, stride)?;
    if kinds.len() != amounts.len() || kinds.len() > MAX_VALUES / 2 {
        return Err(fail("numeric rows require matching bounded page counts"));
    }
    let width = kinds
        .len()
        .checked_mul(2)
        .and_then(|n| n.checked_mul(stride))
        .ok_or_else(|| fail("numeric rows exceed 65536 values"))?;
    if width > MAX_VALUES || cells.len().saturating_mul(width) > MAX_VALUES {
        return Err(fail("numeric rows exceed 65536 values"));
    }
    let mut pages = Vec::with_capacity(kinds.len());
    let mut input_count = 0usize;
    for (kind, amount) in kinds.into_iter().zip(amounts) {
        let kind = kind
            .try_cast::<Array>()
            .ok_or_else(|| fail("numeric rows require array pages"))?;
        let amount = amount
            .try_cast::<Array>()
            .ok_or_else(|| fail("numeric rows require array pages"))?;
        input_count = input_count
            .saturating_add(kind.len())
            .saturating_add(amount.len());
        if kind.len() != amount.len() || input_count > MAX_VALUES {
            return Err(fail("numeric rows require matching bounded page lengths"));
        }
        if cells
            .last()
            .is_some_and(|&cell| (cell + 1) * stride > kind.len())
        {
            return Err(fail("numeric row cell is outside the page"));
        }
        pages.push((kind, amount));
    }
    let mut rows = Vec::with_capacity(cells.len());
    for cell in cells {
        let mut row = Vec::with_capacity(width);
        for (kind, amount) in &pages {
            for slot in cell * stride..(cell + 1) * stride {
                for value in [&kind[slot], &amount[slot]] {
                    // Validate finite numeric input, but keep floats and integers
                    // exactly as supplied instead of applying archive rounding.
                    integer(value)?;
                    row.push(value.clone());
                }
            }
        }
        rows.push(Dynamic::from_array(row));
    }
    Ok(rows)
}

fn numeric_equal(left: Array, right: Array) -> Result<bool> {
    if left.len() > MAX_VALUES || right.len() > MAX_VALUES {
        return Err(fail("numeric comparison exceeds 65536 values"));
    }
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left, right) in left.iter().zip(&right) {
        // Match Rhai's numeric equality without invoking a script operator for
        // each element. Integer pairs retain full precision; mixed pairs use
        // the engine's FLOAT representation. Validate finite numeric input.
        integer(left)?;
        integer(right)?;
        let equal = if left.is::<rhai::INT>() && right.is::<rhai::INT>() {
            left.clone().cast::<rhai::INT>() == right.clone().cast::<rhai::INT>()
        } else {
            let number = |value: &Dynamic| {
                value
                    .as_float()
                    .unwrap_or_else(|_| value.clone().cast::<rhai::INT>() as rhai::FLOAT)
            };
            number(left) == number(right)
        };
        if !equal {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Sparse integer rows in legacy `|`/`,` pages. Keep this pure and bounded so
/// saved graphs can use the same codec in scene hooks and stateless replay.
fn unpack_rows(pages: Array, page_size: rhai::INT) -> Result<rhai::Map> {
    let size = usize::try_from(page_size)
        .ok()
        .filter(|&n| n > 0 && n <= MAX_VALUES)
        .ok_or_else(|| fail("numeric row page size is out of bounds"))?;
    if pages.len() > MAX_VALUES / size {
        return Err(fail("numeric rows exceed 65536 cells"));
    }
    let mut result = rhai::Map::new();
    let mut values = 0usize;
    let mut bytes = 0usize;
    for (page, text) in pages.into_iter().enumerate() {
        let text = text
            .try_cast::<ImmutableString>()
            .ok_or_else(|| fail("numeric rows require text pages"))?;
        bytes = bytes.saturating_add(text.len());
        if bytes > MAX_SCRIPT_BYTES {
            return Err(fail("numeric rows exceed the script string limit"));
        }
        if text.is_empty() {
            continue;
        }
        for (cell, row) in text.split('|').enumerate() {
            if cell >= size {
                return Err(fail("numeric row page has too many cells"));
            }
            if row.is_empty() {
                continue;
            }
            let mut decoded = Array::new();
            for value in row.split(',') {
                values += 1;
                if values > MAX_VALUES {
                    return Err(fail("numeric rows exceed 65536 values"));
                }
                decoded.push(Dynamic::from_int(
                    value
                        .parse::<rhai::INT>()
                        .map_err(|_| fail("numeric row has an invalid integer"))?,
                ));
            }
            result.insert(
                (page * size + cell).to_string().into(),
                Dynamic::from_array(decoded),
            );
        }
    }
    Ok(result)
}

fn pack_rows(rows: rhai::Map, page_size: rhai::INT, page_count: rhai::INT) -> Result<Array> {
    use std::fmt::Write;
    let size = usize::try_from(page_size)
        .ok()
        .filter(|&n| n > 0 && n <= MAX_VALUES)
        .ok_or_else(|| fail("numeric row page size is out of bounds"))?;
    let count = usize::try_from(page_count)
        .ok()
        .filter(|&n| n <= MAX_VALUES / size)
        .ok_or_else(|| fail("numeric rows exceed 65536 cells"))?;
    let mut cells = std::collections::BTreeMap::new();
    let mut values = 0usize;
    let mut bytes = 0usize;
    for (key, row) in rows {
        let cell = key
            .parse::<usize>()
            .ok()
            .filter(|&n| n < size * count && n.to_string() == key)
            .ok_or_else(|| fail("numeric row key is out of bounds or noncanonical"))?;
        let row = row
            .try_cast::<Array>()
            .ok_or_else(|| fail("numeric rows require array values"))?;
        values = values.saturating_add(row.len());
        if values > MAX_VALUES {
            return Err(fail("numeric rows exceed 65536 values"));
        }
        let mut text = String::new();
        for (i, value) in row.iter().enumerate() {
            if i > 0 {
                text.push(',');
            }
            write!(text, "{}", integer(value)?).unwrap();
        }
        bytes = bytes.saturating_add(text.len());
        if bytes > MAX_SCRIPT_BYTES {
            return Err(fail("numeric rows exceed the script string limit"));
        }
        if !text.is_empty() {
            cells.insert(cell, text);
        }
    }
    let mut pages = vec![Dynamic::from(""); count];
    for &cell in cells.keys() {
        let page = cell / size;
        if !pages[page].clone().cast::<ImmutableString>().is_empty() {
            continue;
        }
        let mut text = String::new();
        for slot in page * size..(page + 1) * size {
            if slot % size != 0 {
                text.push('|');
            }
            if let Some(row) = cells.get(&slot) {
                text.push_str(row);
            }
        }
        bytes = bytes.saturating_add(size - 1);
        if bytes > MAX_SCRIPT_BYTES {
            return Err(fail("numeric rows exceed the script string limit"));
        }
        pages[page] = Dynamic::from(text);
    }
    Ok(pages)
}

pub(super) fn register(engine: &mut Engine) {
    engine.register_fn("pack_numeric_cells", pack);
    engine.register_fn("unpack_numeric_cells", unpack);
    engine.register_fn("gather_numeric_pairs", gather_pairs);
    engine.register_fn("numeric_arrays_equal", numeric_equal);
    engine.register_fn("unpack_integer_rows", unpack_rows);
    engine.register_fn("pack_integer_rows", pack_rows);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn array(values: impl IntoIterator<Item = i64>) -> Array {
        values.into_iter().map(Dynamic::from_int).collect()
    }
    fn numbers(values: Array) -> Vec<f32> {
        values.into_iter().map(Dynamic::cast::<f32>).collect()
    }

    #[test]
    fn sparse_cells_compress_runs_and_ignore_unoccupied_values() {
        for stride in [1, 4] {
            for mask in 1usize..64 {
                let cells: Vec<i64> = (0..6).filter(|&i| mask & (1 << i) != 0).collect();
                let values: Vec<i64> = (0..6 * stride).map(|i| (i * 7 % 5) - 2).collect();
                let packed = pack(array(values.clone()), array(cells.clone()), stride).unwrap();
                let decoded =
                    numbers(unpack(packed.clone(), array(cells.clone()), stride).unwrap());
                let expected: Vec<f32> = cells
                    .iter()
                    .flat_map(|&cell| {
                        (cell * stride..(cell + 1) * stride).map(|i| values[i as usize] as f32)
                    })
                    .collect();
                assert_eq!(decoded, expected);
                let whole = numbers(unpack(packed, array(0..6), stride).unwrap());
                for (index, value) in whole.iter().enumerate() {
                    let cell = index as i64 / stride;
                    assert_eq!(
                        *value,
                        if cells.contains(&cell) {
                            values[index] as f32
                        } else {
                            0.
                        }
                    );
                }
            }
        }
        assert_eq!(pack(array([0; 900]), array([1, 2, 3]), 4).unwrap(), "0:900");
        assert_eq!(pack(array([2; 6]), array(0..6), 1).unwrap(), "2:6");
    }

    #[test]
    fn legacy_literal_and_run_pages_decode_the_same_selected_slots() {
        for text in ["0,0,3,3,3,3,0,0,0,0,0,0", "0:2,3:4,0:6"] {
            assert_eq!(
                numbers(unpack(text.into(), array([1, 2, 4]), 2).unwrap()),
                [3., 3., 3., 3., 0., 0.]
            );
        }
        assert_eq!(
            numbers(unpack("".into(), array([0, 224]), 4).unwrap()),
            [0.; 8]
        );
        assert_eq!(pack(array([7; 12]), vec![], 4).unwrap(), "");
    }

    #[test]
    fn native_archive_work_and_allocations_remain_bounded() {
        for (cells, stride) in [
            (array([1, 0]), 1),
            (array([0, 0]), 1),
            (array([-1]), 1),
            (array([65536]), 1),
            (array([0]), 0),
            (array([0]), 65537),
        ] {
            assert!(unpack("0:65536".into(), cells, stride).is_err());
        }
        for text in ["1:0", "1:65537", "1:5:2", "bad", "1", "1:65536,0"] {
            assert!(unpack(text.into(), array([2]), 1).is_err(), "{text}");
        }
        assert!(pack(array([0; 2]), array([2]), 1).is_err());
        assert!(pack(array(std::iter::repeat_n(0, MAX_VALUES + 1)), array([0]), 1).is_err());
        assert!(pack(vec![Dynamic::from_float(f32::NAN)], array([0]), 1).is_err());
        assert_eq!(
            unpack("1:65536".into(), array([65535]), 1).unwrap().len(),
            1
        );
    }

    #[test]
    fn gathered_rows_preserve_sparse_cell_page_slot_order_and_numeric_types() {
        for stride in [1, 4] {
            for mask in 1usize..64 {
                let cells: Vec<i64> = (0..6).filter(|&i| mask & (1 << i) != 0).collect();
                let kinds: Array = (0..3)
                    .map(|page| Dynamic::from_array(array((0..6 * stride).map(|i| page * 100 + i))))
                    .collect();
                let amounts: Array = (0..3)
                    .map(|page| {
                        Dynamic::from_array(
                            (0..6 * stride)
                                .map(|i| Dynamic::from_float((page * 100 + i) as f32 + 0.25))
                                .collect(),
                        )
                    })
                    .collect();
                let rows = gather_pairs(kinds, amounts, array(cells.clone()), stride).unwrap();
                assert_eq!(rows.len(), cells.len());
                for (row, cell) in rows.into_iter().zip(cells) {
                    let row = row.cast::<Array>();
                    let expected: Vec<i64> = (0..3)
                        .flat_map(|page| {
                            (cell * stride..(cell + 1) * stride).map(move |i| page * 100 + i)
                        })
                        .collect();
                    assert_eq!(row.len(), expected.len() * 2);
                    for (pair, expected) in row.chunks_exact(2).zip(expected) {
                        assert_eq!(pair[0].clone().cast::<i64>(), expected);
                        assert_eq!(pair[1].clone().cast::<f32>(), expected as f32 + 0.25);
                    }
                }
            }
        }
        assert!(gather_pairs(vec![], vec![], vec![], 1).unwrap().is_empty());
    }

    #[test]
    fn numeric_row_inputs_and_output_allocations_are_bounded() {
        let page = || vec![Dynamic::from_array(array([1, 2, 3, 4]))];
        for cells in [array([-1]), array([1, 0]), array([0, 0]), array([4])] {
            assert!(gather_pairs(page(), page(), cells, 1).is_err());
        }
        assert!(gather_pairs(page(), vec![], array([0]), 1).is_err());
        assert!(gather_pairs(array([1]), array([1]), array([0]), 1).is_err());
        assert!(
            gather_pairs(page(), vec![Dynamic::from_array(array([1]))], array([0]), 1).is_err()
        );
        for value in [
            Dynamic::from("bad"),
            Dynamic::from_float(f32::NAN),
            Dynamic::from_float(f32::INFINITY),
        ] {
            assert!(
                gather_pairs(
                    vec![Dynamic::from_array(vec![value])],
                    vec![Dynamic::from_array(array([1]))],
                    array([0]),
                    1
                )
                .is_err()
            );
        }
        let large = || {
            vec![Dynamic::from_array(array(std::iter::repeat_n(
                0, MAX_VALUES,
            )))]
        };
        assert!(gather_pairs(large(), large(), array([0]), 1).is_err());
        assert!(gather_pairs(page(), page(), array([0]), 0).is_err());
        assert!(gather_pairs(page(), page(), array([0]), MAX_VALUES as i64).is_err());
    }

    #[test]
    fn sparse_integer_rows_match_legacy_text_and_roundtrip_across_pages() {
        let mut rows = rhai::Map::new();
        rows.insert("0".into(), Dynamic::from_array(array([21, 1, 76])));
        rows.insert("74".into(), Dynamic::from_array(array([9, 0, 0, 76])));
        rows.insert("76".into(), Dynamic::from_array(array([21, 0, 74])));
        let pages = pack_rows(rows.clone(), 75, 867).unwrap();
        assert_eq!(pages.len(), 867);
        assert_eq!(
            pages[0].clone().cast::<String>(),
            format!("21,1,76{}9,0,0,76", "|".repeat(74))
        );
        assert_eq!(
            pages[1].clone().cast::<String>(),
            format!("|21,0,74{}", "|".repeat(73))
        );
        assert!(pages[2].clone().cast::<String>().is_empty());
        let decoded = unpack_rows(pages, 75).unwrap();
        for (key, row) in rows {
            assert_eq!(
                decoded[&key]
                    .clone()
                    .cast::<Array>()
                    .into_iter()
                    .map(Dynamic::cast::<i64>)
                    .collect::<Vec<_>>(),
                row.cast::<Array>()
                    .into_iter()
                    .map(Dynamic::cast::<i64>)
                    .collect::<Vec<_>>()
            );
        }
        assert_eq!(decoded.len(), 3);
        // Legacy short pages also remain readable; packing is canonical.
        assert_eq!(
            unpack_rows(vec![Dynamic::from("|1,-2|3")], 75)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn sparse_integer_rows_reject_unbounded_and_malformed_inputs() {
        for text in ["1,", "bad", "1|2|3"] {
            assert!(unpack_rows(vec![Dynamic::from(text)], 2).is_err());
        }
        assert!(unpack_rows(array([1]), 75).is_err());
        assert!(unpack_rows(vec![], 0).is_err());
        assert!(unpack_rows(vec![Dynamic::from(""); 874], 75).is_err());
        assert!(unpack_rows(vec![Dynamic::from("1,".repeat(MAX_VALUES))], 1).is_err());
        for key in ["-1", "01", "75"] {
            let mut rows = rhai::Map::new();
            rows.insert(key.into(), Dynamic::from_array(array([1])));
            assert!(pack_rows(rows, 75, 1).is_err());
        }
        assert!(pack_rows(rhai::Map::new(), 75, 874).is_err());
        let mut rows = rhai::Map::new();
        rows.insert(
            "0".into(),
            Dynamic::from_array(array(std::iter::repeat_n(0, MAX_VALUES + 1))),
        );
        assert!(pack_rows(rows, 75, 1).is_err());
    }

    #[test]
    fn numeric_comparison_matches_interpreter_equality_and_bounds_work() {
        let mut engine = Engine::new();
        register(&mut engine);
        for (left, right) in [
            ("[1,2,3]", "[1.0,2.0,3.0]"),
            ("[]", "[]"),
            ("[1]", "[1,2]"),
            ("[1,2]", "[1,3]"),
            ("[-0.0]", "[0]"),
            ("[9007199254740992]", "[9007199254740993]"),
        ] {
            assert!(
                engine
                    .eval::<bool>(&format!(
                        "({left} == {right}) == numeric_arrays_equal({left},{right})"
                    ))
                    .unwrap()
            );
        }
        assert!(numeric_equal(array(std::iter::repeat_n(0, MAX_VALUES + 1)), vec![]).is_err());
        assert!(numeric_equal(vec![Dynamic::from_float(f32::NAN)], array([0])).is_err());
    }
}
