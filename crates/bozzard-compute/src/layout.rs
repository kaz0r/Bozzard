use anyhow::{Context, Result, bail, ensure};
use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scalar {
    F32,
    I32,
    U32,
}

/// Layout comes from validated WGSL. Offsets and strides include WGSL padding.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Layout {
    pub(crate) size: u32,
    pub(crate) alignment: u32,
    pub(crate) shape: Shape,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    Scalar(Scalar),
    Vector {
        scalar: Scalar,
        lanes: u32,
    },
    /// Values are an array of columns, each an array of rows.
    Matrix {
        columns: u32,
        rows: u32,
        stride: u32,
    },
    Array {
        element: Box<Layout>,
        count: Option<u32>,
        stride: u32,
    },
    Struct(Vec<(String, u32, Layout)>),
}

impl Layout {
    pub fn shape(&self) -> &Shape {
        &self.shape
    }
    pub fn alignment(&self) -> u32 {
        self.alignment
    }
    pub fn minimum_size(&self) -> u32 {
        self.size
    }
    pub fn is_dynamic(&self) -> bool {
        match &self.shape {
            Shape::Array { count: None, .. } => true,
            Shape::Struct(fields) => fields.last().is_some_and(|(_, _, ty)| ty.is_dynamic()),
            _ => false,
        }
    }
    /// Size with `elements` in a runtime array. Fixed layouts require one complete value.
    pub fn buffer_size(&self, elements: u32) -> Result<usize> {
        ensure!(elements > 0, "compute buffers need at least one element");
        let size = match &self.shape {
            Shape::Array {
                count: None,
                stride,
                ..
            } => u64::from(*stride) * u64::from(elements),
            Shape::Struct(fields) if self.is_dynamic() => {
                let (_, offset, tail) = fields.last().unwrap();
                u64::from(*offset) + tail.buffer_size(elements)? as u64
            }
            _ => {
                ensure!(
                    elements == 1,
                    "fixed WGSL layouts require a single complete value"
                );
                u64::from(self.size)
            }
        };
        ensure!(
            size >= u64::from(self.size),
            "runtime array allocation is shorter than the WGSL minimum binding size of {} bytes; allocate more elements",
            self.size
        );
        ensure!(
            size <= crate::MAX_BUFFER_BYTES as u64,
            "compute buffer exceeds 64 MiB"
        );
        Ok(size as usize)
    }
    pub fn pack(&self, value: &Value) -> Result<Vec<u8>> {
        let count = self.runtime_count(value)?;
        let mut bytes = vec![0; self.buffer_size(count)?];
        self.write(value, &mut bytes)?;
        Ok(bytes)
    }
    /// A typed contiguous range of a top-level array. Offsets include padded element strides.
    pub fn array_range(&self, elements: u32, first: u32, length: u32) -> Result<(u64, Layout)> {
        let Shape::Array {
            element,
            count,
            stride,
        } = &self.shape
        else {
            bail!("range operations require a top-level WGSL array");
        };
        let capacity = count.unwrap_or(elements);
        ensure!(
            length > 0 && first.checked_add(length).is_some_and(|end| end <= capacity),
            "compute array range is out of bounds"
        );
        let size = stride
            .checked_mul(length)
            .context("compute array range overflow")?;
        ensure!(
            size as usize <= crate::MAX_BUFFER_BYTES,
            "compute array range exceeds buffer limit"
        );
        Ok((
            u64::from(first) * u64::from(*stride),
            Layout {
                size,
                alignment: self.alignment,
                shape: Shape::Array {
                    element: element.clone(),
                    count: Some(length),
                    stride: *stride,
                },
            },
        ))
    }
    fn runtime_count(&self, value: &Value) -> Result<u32> {
        match &self.shape {
            Shape::Array { count: None, .. } => Ok(u32::try_from(
                value.as_array().context("expected an array")?.len(),
            )?),
            Shape::Struct(fields) if self.is_dynamic() => {
                let (name, _, ty) = fields.last().unwrap();
                ty.runtime_count(value.get(name).context("missing runtime array field")?)
            }
            _ => Ok(1),
        }
    }
    fn write(&self, value: &Value, bytes: &mut [u8]) -> Result<()> {
        match &self.shape {
            Shape::Scalar(scalar) => write_scalar(*scalar, value, bytes)?,
            Shape::Vector { scalar, lanes } => {
                let values = array(value, *lanes as usize)?;
                for (index, value) in values.iter().enumerate() {
                    write_scalar(*scalar, value, &mut bytes[index * 4..index * 4 + 4])?;
                }
            }
            Shape::Matrix {
                columns,
                rows,
                stride,
            } => {
                for (column, values) in array(value, *columns as usize)?.iter().enumerate() {
                    for (row, value) in array(values, *rows as usize)?.iter().enumerate() {
                        let start = column * *stride as usize + row * 4;
                        write_scalar(Scalar::F32, value, &mut bytes[start..start + 4])?;
                    }
                }
            }
            Shape::Array {
                element,
                count,
                stride,
            } => {
                let count = count.map_or(bytes.len() / *stride as usize, |n| n as usize);
                for (index, value) in array(value, count)?.iter().enumerate() {
                    let start = index * *stride as usize;
                    element
                        .write(value, &mut bytes[start..start + element.size as usize])
                        .with_context(|| format!("array element {index}"))?;
                }
            }
            Shape::Struct(fields) => {
                let values = value.as_object().context("expected a field map")?;
                ensure!(
                    values.len() == fields.len(),
                    "WGSL struct fields do not match"
                );
                for (name, offset, ty) in fields {
                    let field = values
                        .get(name)
                        .with_context(|| format!("missing field '{name}'"))?;
                    let start = *offset as usize;
                    let end = if ty.is_dynamic() {
                        bytes.len()
                    } else {
                        start + ty.size as usize
                    };
                    ty.write(field, &mut bytes[start..end])
                        .with_context(|| format!("field '{name}'"))?;
                }
            }
        }
        Ok(())
    }
    /// Decode a readback with a caller-supplied scalar budget. Padding is never exposed.
    pub fn unpack(&self, bytes: &[u8], mut max_values: usize) -> Result<Value> {
        ensure!(
            bytes.len() >= self.size as usize,
            "readback is shorter than the WGSL layout"
        );
        if let Some((offset, stride)) = self.dynamic_tail() {
            ensure!(
                (bytes.len() - offset as usize).is_multiple_of(stride as usize),
                "readback ends inside an array element"
            );
        } else {
            ensure!(
                bytes.len() == self.size as usize,
                "readback size does not match WGSL layout"
            );
        }
        self.read(bytes, &mut max_values)
    }
    fn dynamic_tail(&self) -> Option<(u32, u32)> {
        match &self.shape {
            Shape::Array {
                count: None,
                stride,
                ..
            } => Some((0, *stride)),
            Shape::Struct(fields) => fields
                .last()
                .and_then(|(_, offset, ty)| ty.dynamic_tail().map(|(o, s)| (offset + o, s))),
            _ => None,
        }
    }
    fn read(&self, bytes: &[u8], remaining: &mut usize) -> Result<Value> {
        Ok(match &self.shape {
            Shape::Scalar(scalar) => read_scalar(*scalar, bytes, remaining)?,
            Shape::Vector { scalar, lanes } => Value::Array(
                (0..*lanes as usize)
                    .map(|i| read_scalar(*scalar, &bytes[i * 4..i * 4 + 4], remaining))
                    .collect::<Result<_>>()?,
            ),
            Shape::Matrix {
                columns,
                rows,
                stride,
            } => Value::Array(
                (0..*columns as usize)
                    .map(|column| {
                        Ok(Value::Array(
                            (0..*rows as usize)
                                .map(|row| {
                                    let start = column * *stride as usize + row * 4;
                                    read_scalar(Scalar::F32, &bytes[start..start + 4], remaining)
                                })
                                .collect::<Result<_>>()?,
                        ))
                    })
                    .collect::<Result<_>>()?,
            ),
            Shape::Array {
                element,
                count,
                stride,
            } => {
                let count = count.map_or(bytes.len() / *stride as usize, |n| n as usize);
                ensure!(count <= *remaining, "readback value budget exceeded");
                Value::Array(
                    (0..count)
                        .map(|index| {
                            let start = index * *stride as usize;
                            element.read(&bytes[start..start + element.size as usize], remaining)
                        })
                        .collect::<Result<_>>()?,
                )
            }
            Shape::Struct(fields) => {
                let mut values = Map::new();
                for (name, offset, ty) in fields {
                    let start = *offset as usize;
                    let end = if ty.is_dynamic() {
                        bytes.len()
                    } else {
                        start + ty.size as usize
                    };
                    values.insert(name.clone(), ty.read(&bytes[start..end], remaining)?);
                }
                Value::Object(values)
            }
        })
    }
}

fn array(value: &Value, length: usize) -> Result<&[Value]> {
    let values = value.as_array().context("expected an array")?;
    ensure!(
        values.len() == length,
        "expected {length} values, got {}",
        values.len()
    );
    Ok(values)
}
fn write_scalar(scalar: Scalar, value: &Value, bytes: &mut [u8]) -> Result<()> {
    let n = value.as_f64().context("expected a number")?;
    ensure!(n.is_finite(), "compute values must be finite");
    let raw = match scalar {
        Scalar::F32 => {
            let v = n as f32;
            ensure!(v.is_finite(), "value exceeds f32 range");
            v.to_le_bytes()
        }
        Scalar::I32 => {
            ensure!(
                n.fract() == 0. && n >= i32::MIN as f64 && n <= i32::MAX as f64,
                "expected an i32 integer"
            );
            (n as i32).to_le_bytes()
        }
        Scalar::U32 => {
            ensure!(
                n.fract() == 0. && n >= 0. && n <= u32::MAX as f64,
                "expected a u32 integer"
            );
            (n as u32).to_le_bytes()
        }
    };
    bytes[..4].copy_from_slice(&raw);
    Ok(())
}
fn read_scalar(scalar: Scalar, bytes: &[u8], remaining: &mut usize) -> Result<Value> {
    ensure!(*remaining > 0, "readback value budget exceeded");
    *remaining -= 1;
    let raw: [u8; 4] = bytes[..4].try_into().unwrap();
    Ok(match scalar {
        Scalar::F32 => {
            let n = f32::from_le_bytes(raw);
            if !n.is_finite() {
                bail!("GPU result contains a non-finite f32");
            }
            Value::from(n)
        }
        Scalar::I32 => Value::from(i32::from_le_bytes(raw)),
        Scalar::U32 => Value::from(u32::from_le_bytes(raw)),
    })
}

pub(crate) fn reflect(
    module: &naga::Module,
    layouter: &naga::proc::Layouter,
    ty: naga::Handle<naga::Type>,
    depth: usize,
) -> Result<Layout> {
    reflect_inner(module, layouter, ty, depth, &mut 4096)
}
fn reflect_inner(
    module: &naga::Module,
    layouter: &naga::proc::Layouter,
    ty: naga::Handle<naga::Type>,
    depth: usize,
    budget: &mut usize,
) -> Result<Layout> {
    use naga::TypeInner as T;
    ensure!(depth < 16, "compute data layout exceeds 16 levels");
    ensure!(*budget > 0, "compute data layout exceeds 4096 fields");
    *budget -= 1;
    let mut recurse = |t| reflect_inner(module, layouter, t, depth + 1, budget);
    let shape = match &module.types[ty].inner {
        T::Scalar(s) | T::Atomic(s) => Shape::Scalar(scalar(*s)?),
        T::Vector { size, scalar: s } => Shape::Vector {
            scalar: scalar(*s)?,
            lanes: *size as u32,
        },
        T::Matrix {
            columns,
            rows,
            scalar: s,
        } => {
            ensure!(
                scalar(*s)? == Scalar::F32,
                "compute matrices must contain f32 values"
            );
            Shape::Matrix {
                columns: *columns as u32,
                rows: *rows as u32,
                stride: if *rows == naga::VectorSize::Bi { 8 } else { 16 },
            }
        }
        T::Array { base, size, stride } => Shape::Array {
            element: Box::new(recurse(*base)?),
            count: match size {
                naga::ArraySize::Constant(n) => Some(n.get()),
                naga::ArraySize::Dynamic => None,
                _ => bail!("override-sized compute arrays are not supported"),
            },
            stride: *stride,
        },
        T::Struct { members, .. } => Shape::Struct(
            members
                .iter()
                .map(|member| {
                    Ok((
                        member
                            .name
                            .clone()
                            .context("compute struct fields need names")?,
                        member.offset,
                        recurse(member.ty)?,
                    ))
                })
                .collect::<Result<_>>()?,
        ),
        _ => {
            bail!("compute buffers support numeric scalars, vectors, matrices, arrays and structs")
        }
    };
    let size = layouter[ty].size;
    ensure!(
        size > 0 && size as usize <= crate::MAX_BUFFER_BYTES,
        "compute data layout exceeds the buffer limit"
    );
    Ok(Layout {
        size,
        alignment: layouter[ty].alignment * 1,
        shape,
    })
}
fn scalar(s: naga::Scalar) -> Result<Scalar> {
    ensure!(s.width == 4, "compute data supports 32-bit numbers");
    match s.kind {
        naga::ScalarKind::Float => Ok(Scalar::F32),
        naga::ScalarKind::Sint => Ok(Scalar::I32),
        naga::ScalarKind::Uint => Ok(Scalar::U32),
        _ => bail!("compute data supports f32, i32 and u32"),
    }
}
