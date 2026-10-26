//! Allocation-free admission of Dosty's plain-text subset of Yjs update V1.
//! Yrs remains the decoder and CRDT implementation. Preflight bounds counts
//! BEFORE allocation, rejects nested/embedded data, bad IDs and trailing bytes.
//! Keep this validator pinned to the Yrs wire version; never partially accept an
//! update. Pending structures are checked too, before they can be integrated.

use yrs::encoding::read::{Cursor, Error, Read};

use crate::document::collaboration::{CollaborationError, MAX_STATE_BYTES};

pub(super) const MAX_STRUCTURES: usize = 200_000;

pub(super) fn validate(bytes: &[u8]) -> Result<(), CollaborationError> {
    if bytes.len() > MAX_STATE_BYTES {
        return Err(CollaborationError::TooLarge);
    }
    structure_count(bytes).map(|_| ())
}
pub(super) fn structure_count(bytes: &[u8]) -> Result<usize, CollaborationError> {
    validate_v1(bytes).map_err(|error| CollaborationError::InvalidData(error.to_string()))
}
fn count(cursor: &mut Cursor<'_>, budget: &mut usize) -> Result<u32, Error> {
    let count: u32 = cursor.read_var()?;
    if count as usize > *budget || count as usize > cursor.buf.len() - cursor.next {
        return Err(Error::UnexpectedValue);
    }
    *budget -= count as usize;
    Ok(count)
}
fn client(cursor: &mut Cursor<'_>) -> Result<u64, Error> {
    let client: u64 = cursor.read_var()?;
    if client >= 1 << 53 {
        return Err(Error::UnexpectedValue);
    }
    Ok(client)
}
fn id(cursor: &mut Cursor<'_>) -> Result<(), Error> {
    client(cursor)?;
    let _: u32 = cursor.read_var()?;
    Ok(())
}
fn validate_v1(bytes: &[u8]) -> Result<usize, Error> {
    let mut cursor = Cursor::new(bytes);
    let mut budget = MAX_STRUCTURES;
    let clients = count(&mut cursor, &mut budget)?;
    for _ in 0..clients {
        let blocks = count(&mut cursor, &mut budget)?;
        client(&mut cursor)?;
        let mut clock: u32 = cursor.read_var()?;
        for _ in 0..blocks {
            let info = cursor.read_u8()?;
            let length = if info == 0 || info == 10 {
                cursor.read_var::<u32>()?
            } else {
                // Only plain string and deleted structs. Parent-sub attributes,
                // embeds, arrays, rich formatting and subdocuments are unsupported.
                let kind = info & 0x1f;
                if (kind != 4 && kind != 1) || info & 0x20 != 0 {
                    return Err(Error::UnexpectedValue);
                }
                if info & 0x80 != 0 {
                    id(&mut cursor)?;
                }
                if info & 0x40 != 0 {
                    id(&mut cursor)?;
                }
                if info & 0xc0 == 0 {
                    let parent: u32 = cursor.read_var()?;
                    if parent != 1 || cursor.read_string()? != "markdown" {
                        return Err(Error::UnexpectedValue);
                    }
                }
                if kind == 4 {
                    cursor
                        .read_string()?
                        .encode_utf16()
                        .count()
                        .try_into()
                        .map_err(|_| Error::UnexpectedValue)?
                } else {
                    cursor.read_var::<u32>()?
                }
            };
            if length == 0 {
                return Err(Error::UnexpectedValue);
            }
            clock = clock.checked_add(length).ok_or(Error::UnexpectedValue)?;
        }
    }
    let clients = count(&mut cursor, &mut budget)?;
    for _ in 0..clients {
        client(&mut cursor)?;
        let ranges = count(&mut cursor, &mut budget)?;
        for _ in 0..ranges {
            let clock: u32 = cursor.read_var()?;
            let length: u32 = cursor.read_var()?;
            clock.checked_add(length).ok_or(Error::UnexpectedValue)?;
        }
    }
    if cursor.next != bytes.len() {
        return Err(Error::UnexpectedValue);
    }
    Ok(MAX_STRUCTURES - budget)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_counts_identifiers_and_random_input_are_controlled_errors() {
        for bytes in [
            &[255, 255, 255, 127][..],
            &[1, 1, 255, 255, 255, 255, 255, 255, 255, 127, 0][..],
            &[0, 0, 1][..],
        ] {
            assert!(validate(bytes).is_err());
        }
        let mut seed = 0x8ea136f240e1acbdu64;
        for length in 0..256 {
            let bytes: Vec<_> = (0..length)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    seed as u8
                })
                .collect();
            assert!(validate(&bytes).is_err());
        }
    }
}
