pub(super) struct CopyRegion<'a, T> {
    pub(super) source: &'a T,
    pub(super) source_offset: u64,
    pub(super) destination_offset: u64,
    pub(super) bytes: u64,
}

/// Combines consecutive copies to a common destination without changing the
/// source or destination bytes of any piece. Source references identify the
/// resources; the caller keeps the original pieces for CPU-side copy-back.
pub(super) fn coalesce<'a, T: 'a>(
    pieces: impl Iterator<Item = CopyRegion<'a, T>>,
) -> impl Iterator<Item = CopyRegion<'a, T>> {
    let mut pieces = pieces.peekable();
    std::iter::from_fn(move || {
        let mut copy = pieces.next()?;
        while let Some(next) = pieces.peek() {
            if !std::ptr::eq(copy.source, next.source)
                || copy.source_offset.checked_add(copy.bytes) != Some(next.source_offset)
                || copy.destination_offset.checked_add(copy.bytes) != Some(next.destination_offset)
            {
                break;
            }
            let Some(source_end) = next.source_offset.checked_add(next.bytes) else {
                break;
            };
            if next.destination_offset.checked_add(next.bytes).is_none() {
                break;
            }
            copy.bytes = source_end - copy.source_offset;
            let _ = pieces.next();
        }
        Some(copy)
    })
}

#[cfg(test)]
mod tests {
    use super::{CopyRegion, coalesce};

    fn commands<const N: usize>(pieces: [(&u8, u64, u64, u64); N]) -> Vec<(u8, u64, u64, u64)> {
        coalesce(
            pieces
                .into_iter()
                .map(
                    |(source, source_offset, destination_offset, bytes)| CopyRegion {
                        source,
                        source_offset,
                        destination_offset,
                        bytes,
                    },
                ),
        )
        .map(|c| (*c.source, c.source_offset, c.destination_offset, c.bytes))
        .collect()
    }

    #[test]
    fn readback_coalesces_a_run_of_adjacent_pieces() {
        let a = 1;
        assert!(commands([]).is_empty());
        assert_eq!(commands([(&a, 7, 3, 5)]), [(1, 7, 3, 5)]);
        assert_eq!(
            commands([(&a, 7, 3, 5), (&a, 12, 8, 8), (&a, 20, 16, 2)]),
            [(1, 7, 3, 15)]
        );
    }

    #[test]
    fn readback_stops_at_gaps_overlaps_and_resource_changes() {
        let (a, b) = (1, 2);
        assert_eq!(
            commands([
                (&a, 0, 0, 4),
                (&a, 5, 4, 4),   // Source gap.
                (&a, 9, 9, 4),   // Destination gap.
                (&a, 11, 13, 4), // Source overlap.
                (&a, 15, 15, 4), // Destination overlap.
                (&b, 19, 19, 4), // Different source with matching offsets.
                (&a, 19, 23, 4), // Returning to the first source preserves order.
                (&a, 23, 27, 2),
            ]),
            [
                (1, 0, 0, 4),
                (1, 5, 4, 4),
                (1, 9, 9, 4),
                (1, 11, 13, 4),
                (1, 15, 15, 4),
                (2, 19, 19, 4),
                (1, 19, 23, 6)
            ]
        );
    }

    #[test]
    fn readback_rejects_wrapping_offsets() {
        let a = 1;
        assert_eq!(
            commands([(&a, u64::MAX - 1, 0, 1), (&a, u64::MAX, 1, 1)]),
            [(1, u64::MAX - 1, 0, 1), (1, u64::MAX, 1, 1)]
        );
        assert_eq!(
            commands([(&a, 0, u64::MAX - 1, 1), (&a, 1, u64::MAX, 1)]),
            [(1, 0, u64::MAX - 1, 1), (1, 1, u64::MAX, 1)]
        );
    }
}
