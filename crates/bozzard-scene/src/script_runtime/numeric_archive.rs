//! Bounded numeric archive packing, shared by stateless replay and scene hooks.
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

pub(super) fn register(engine: &mut Engine) {
    engine.register_fn("pack_numeric_cells", pack);
    engine.register_fn("unpack_numeric_cells", unpack);
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
}
