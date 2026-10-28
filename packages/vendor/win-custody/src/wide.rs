//! UTF-16 conversions. A NUL-terminated buffer is only ever read up to a
//! caller-supplied maximum, so a missing terminator cannot turn into a read
//! past the allocation.
#![allow(unsafe_code)]

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;

/// Encodes `value` as NUL-terminated UTF-16 for a Win32 `LPCWSTR` argument.
/// An interior NUL would silently truncate the string on the Win32 side, so
/// it is rejected.
pub(crate) fn wide_null(value: impl AsRef<OsStr>) -> io::Result<Vec<u16>> {
    let mut units: Vec<u16> = value.as_ref().encode_wide().collect();
    if units.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "string contains an interior NUL",
        ));
    }
    units.push(0);
    Ok(units)
}

/// Reads a NUL-terminated UTF-16 string of at most `maximum` units from
/// `pointer`. Returns `None` for a null pointer or when no terminator appears
/// inside the bound, so callers see "unreadable" rather than a truncated value.
///
/// # Safety
///
/// `pointer` must either be null or point at readable memory that stays valid
/// for `maximum` UTF-16 units or up to and including its NUL terminator,
/// whichever comes first.
pub(crate) unsafe fn bounded_units<'a>(pointer: *const u16, maximum: usize) -> Option<&'a [u16]> {
    if pointer.is_null() {
        return None;
    }
    let mut length = 0_usize;
    while length < maximum {
        let unit = pointer.wrapping_add(length);
        // SAFETY: the caller guarantees the buffer is readable up to `maximum`
        // units or its terminator; the loop stops at whichever comes first,
        // so `unit` is inside the readable range.
        if unsafe { *unit } == 0 {
            break;
        }
        length += 1;
    }
    if length == maximum {
        return None;
    }
    // SAFETY: every unit in `..length` was just read through the same pointer.
    Some(unsafe { std::slice::from_raw_parts(pointer, length) })
}

/// Converts a bounded UTF-16 read into a `String`, rejecting invalid UTF-16.
///
/// # Safety
///
/// Same contract as [`bounded_units`].
pub(crate) unsafe fn bounded_string(pointer: *const u16, maximum: usize) -> Option<String> {
    // SAFETY: forwarded verbatim to the caller's guarantee.
    unsafe { bounded_units(pointer, maximum) }.and_then(|units| String::from_utf16(units).ok())
}

#[cfg(test)]
mod tests {
    use super::{bounded_units, wide_null};

    #[test]
    fn bounded_reads_stop_at_the_terminator_and_reject_missing_ones() {
        let terminated: Vec<u16> = "abc".encode_utf16().chain(Some(0)).collect();
        // SAFETY: the buffer is a live Vec with a terminator inside the bound.
        let read = unsafe { bounded_units(terminated.as_ptr(), 8) }.unwrap();
        assert_eq!(String::from_utf16_lossy(read), "abc");

        let unterminated: Vec<u16> = "abcd".encode_utf16().collect();
        // SAFETY: the bound equals the buffer length, so no unit past it is read.
        assert!(unsafe { bounded_units(unterminated.as_ptr(), 4) }.is_none());
        // SAFETY: a null pointer is never dereferenced.
        assert!(unsafe { bounded_units(std::ptr::null(), 4) }.is_none());
    }

    #[test]
    fn wide_null_appends_a_terminator_and_rejects_interior_nuls() {
        assert_eq!(wide_null("x").unwrap(), [u16::from(b'x'), 0]);
        assert!(wide_null("a\0b").is_err());
    }
}
