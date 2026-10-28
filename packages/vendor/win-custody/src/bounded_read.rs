//! Bounded storage for Win32 APIs that report a required byte count.
//!
//! Many Win32 queries are called twice: once with no buffer to learn the size,
//! then again with a buffer of that size. The size can change between the two
//! calls, and a hostile or buggy provider can report a length larger than the
//! buffer it was given. [`probe_word_aligned`] runs that protocol with a cap
//! on every length and exposes only the bytes the API reported writing.
#![allow(unsafe_code)]

use std::io;
use std::mem::{align_of, size_of};
use std::ptr::null_mut;

/// The result of one call that writes into a caller-owned buffer.
#[derive(Debug)]
pub(crate) enum CallOutcome {
    Complete,
    MoreData(io::Error),
}

/// A failure in the null-probe-then-fill protocol.
#[derive(Debug)]
pub(crate) enum ProbeError {
    Call(io::Error),
    ProbeCompleted(io::Error),
    SizeOutOfRange,
    FillNeedsMore(io::Error),
    ReturnedLengthOutOfRange,
}

impl ProbeError {
    /// Folds the protocol failure into an `io::Error`, naming `what` was read.
    pub(crate) fn into_io(self, what: &str) -> io::Error {
        match self {
            Self::Call(error) | Self::ProbeCompleted(error) | Self::FillNeedsMore(error) => error,
            Self::SizeOutOfRange => io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{what} size is out of range"),
            ),
            Self::ReturnedLengthOutOfRange => io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{what} changed during the read"),
            ),
        }
    }
}

/// Word-aligned storage whose logical length is the byte count reported by the
/// API, not the allocation's rounded byte capacity.
#[derive(Debug)]
pub(crate) struct WordAlignedBuffer {
    words: Vec<usize>,
    byte_length: usize,
}

impl WordAlignedBuffer {
    fn zeroed(byte_capacity: usize) -> Self {
        Self {
            words: vec![0_usize; byte_capacity.div_ceil(size_of::<usize>())],
            byte_length: byte_capacity,
        }
    }

    fn pointer(&mut self) -> *mut usize {
        if self.words.is_empty() {
            null_mut()
        } else {
            self.words.as_mut_ptr()
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.byte_length
    }

    pub(crate) fn as_ptr(&self) -> *const u8 {
        self.words.as_ptr().cast()
    }

    fn supports<T>(&self) -> bool {
        align_of::<T>() <= align_of::<usize>() && self.byte_length >= size_of::<T>()
    }

    /// Reads a fixed header after the caller validates the byte representation.
    ///
    /// # Safety
    ///
    /// The reported bytes at the start of the buffer must form a valid `T`.
    pub(crate) unsafe fn read<T: Copy>(&self) -> Option<T> {
        if !self.supports::<T>() {
            return None;
        }
        // SAFETY: `words` supplies at least `usize` alignment, `supports`
        // checked T's alignment and complete byte range, and the caller
        // guarantees that those initialized bytes form a valid T.
        Some(unsafe { self.words.as_ptr().cast::<T>().read() })
    }
}

/// Calls an API first with a null buffer and then with word-aligned storage of
/// the size it asked for. No buffer is exposed unless the required and the
/// returned lengths both fall within `minimum_bytes..=maximum_bytes`.
pub(crate) fn probe_word_aligned(
    minimum_bytes: usize,
    maximum_bytes: usize,
    mut call: impl FnMut(*mut usize, u32, &mut u32) -> io::Result<CallOutcome>,
) -> Result<WordAlignedBuffer, ProbeError> {
    let maximum_bytes = maximum_bytes.min(u32::MAX as usize);
    let mut needed = 0_u32;
    match call(null_mut(), 0, &mut needed).map_err(ProbeError::Call)? {
        CallOutcome::Complete if needed == 0 && minimum_bytes == 0 => {
            return Ok(WordAlignedBuffer::zeroed(0));
        }
        CallOutcome::Complete => {
            return Err(ProbeError::ProbeCompleted(io::Error::last_os_error()));
        }
        CallOutcome::MoreData(_) => {}
    }

    let needed = needed as usize;
    if needed < minimum_bytes || needed > maximum_bytes {
        return Err(ProbeError::SizeOutOfRange);
    }

    // The allocation is zero-initialized, so rounded word padding and any
    // bytes the API does not overwrite never contain uninitialized data.
    let mut storage = WordAlignedBuffer::zeroed(needed);
    let capacity = needed as u32;
    let mut filled = capacity;
    match call(storage.pointer(), capacity, &mut filled).map_err(ProbeError::Call)? {
        CallOutcome::Complete => {}
        // A larger requirement after the probe is a two-call race; the short
        // allocation is discarded without exposing it.
        CallOutcome::MoreData(error) => return Err(ProbeError::FillNeedsMore(error)),
    }

    let filled = filled as usize;
    if filled < minimum_bytes || filled > needed {
        return Err(ProbeError::ReturnedLengthOutOfRange);
    }
    storage.byte_length = filled;
    Ok(storage)
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::mem::{align_of, size_of};

    use super::{probe_word_aligned, CallOutcome, ProbeError};

    fn more_data() -> CallOutcome {
        CallOutcome::MoreData(io::Error::from_raw_os_error(234))
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct Header {
        first: usize,
        second: u32,
    }

    #[test]
    fn the_fill_pointer_and_the_header_are_word_aligned() {
        let expected = Header {
            first: 0x1234,
            second: 0x5678,
        };
        let response = probe_word_aligned(
            size_of::<Header>(),
            size_of::<Header>(),
            |buffer, capacity, needed| {
                if buffer.is_null() {
                    *needed = size_of::<Header>() as u32;
                    return Ok(more_data());
                }
                assert_eq!((buffer as usize) % align_of::<usize>(), 0);
                assert_eq!(capacity as usize, size_of::<Header>());
                // SAFETY: the word-aligned allocation has room for Header.
                unsafe { buffer.cast::<Header>().write(expected) };
                Ok(CallOutcome::Complete)
            },
        )
        .unwrap();

        assert_eq!(response.len(), size_of::<Header>());
        // SAFETY: the fake API wrote `expected` as a complete Header.
        assert_eq!(unsafe { response.read::<Header>() }, Some(expected));
    }

    #[test]
    fn growth_between_the_two_calls_is_rejected() {
        let mut calls = 0;
        let error = probe_word_aligned(1, 64, |buffer, capacity, needed| {
            calls += 1;
            if buffer.is_null() {
                *needed = 8;
                return Ok(more_data());
            }
            assert_eq!(capacity, 8);
            *needed = 16;
            Ok(more_data())
        })
        .unwrap_err();

        assert_eq!(calls, 2);
        assert!(matches!(error, ProbeError::FillNeedsMore(_)));
    }

    #[test]
    fn the_caller_cap_is_enforced_before_allocation() {
        let error = probe_word_aligned(0, 4, |_, _, needed| {
            *needed = 5;
            Ok(more_data())
        })
        .unwrap_err();
        assert!(matches!(error, ProbeError::SizeOutOfRange));
    }

    #[test]
    fn an_api_cannot_report_more_bytes_than_it_was_given() {
        let error = probe_word_aligned(1, 64, |buffer, _, needed| {
            if buffer.is_null() {
                *needed = 8;
                return Ok(more_data());
            }
            *needed = 9;
            Ok(CallOutcome::Complete)
        })
        .unwrap_err();
        assert!(matches!(error, ProbeError::ReturnedLengthOutOfRange));
    }

    #[test]
    fn a_header_larger_than_the_reported_bytes_is_not_read() {
        let response = probe_word_aligned(1, 64, |buffer, _, needed| {
            if buffer.is_null() {
                *needed = 4;
                return Ok(more_data());
            }
            *needed = 4;
            Ok(CallOutcome::Complete)
        })
        .unwrap();
        // SAFETY: `read` refuses before touching memory because 4 bytes cannot
        // hold a Header.
        assert_eq!(unsafe { response.read::<Header>() }, None);
    }
}
