#[derive(Debug, Clone)]
pub enum WireValue {
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct WireField {
    pub value: WireValue,
}

pub fn decode(bytes: &[u8]) -> Option<Vec<WireField>> {
    let mut fields = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let tag = read_varint(bytes, &mut offset)?;
        let number = u32::try_from(tag >> 3).ok()?;
        if number == 0 {
            return None;
        }
        let wire_type = tag & 7;
        let value = match wire_type {
            0 => {
                read_varint(bytes, &mut offset)?;
                continue;
            }
            1 => {
                offset = offset.checked_add(8)?;
                if offset > bytes.len() {
                    return None;
                }
                continue;
            }
            2 => {
                let length = usize::try_from(read_varint(bytes, &mut offset)?).ok()?;
                let end = offset.checked_add(length)?;
                let value = bytes.get(offset..end)?.to_vec();
                offset = end;
                WireValue::Bytes(value)
            }
            5 => {
                offset = offset.checked_add(4)?;
                if offset > bytes.len() {
                    return None;
                }
                continue;
            }
            _ => return None,
        };
        fields.push(WireField { value });
    }
    Some(fields)
}

fn read_varint(bytes: &[u8], offset: &mut usize) -> Option<u64> {
    let mut result = 0_u64;
    for shift in (0..70).step_by(7) {
        let byte = *bytes.get(*offset)?;
        *offset = offset.checked_add(1)?;
        if shift == 63 && byte > 1 {
            return None;
        }
        result |= u64::from(byte & 0x7f).checked_shl(shift)?;
        if byte & 0x80 == 0 {
            return Some(result);
        }
    }
    None
}
