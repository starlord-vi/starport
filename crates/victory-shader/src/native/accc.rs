// SPDX-License-Identifier: GPL-3.0-or-later
// FSPX metadata layout adapted from codec-d--cool_fox-OPTIMIZED
// https://git.gtahub.st/Mskh/codec-d--cool_fox-OPTIMIZED
// 808342e93a268ded13e67f4aca016214c6d3230d, native.rs (inherited contributors).
// Starport: checked tables, duplicate/root consistency checks and regressions.
//! Native compute programs produced by ACO, in ACCC/FSPX v1 containers.

use super::{
    ComputeIo, HwStage, NativeProgram, ResourceType, RootParam, RootType, StageIo, UsedResource,
};
use crate::ShaderError::{self, Malformed};
use crate::dxbc::u32_at;

pub(super) fn program(part: &[u8]) -> Result<NativeProgram, ShaderError> {
    if part.get(..4) != Some(b"FSPX") || u32_at(part, 4)? != 1 {
        return Err(Malformed("unknown FSPX version"));
    }
    let size = u32_at(part, 12)? as usize;
    let part = part
        .get(..size)
        .filter(|p| p.len() >= 16)
        .ok_or(Malformed("FSPX size out of bounds"))?;
    let count = u32_at(part, 8)? as usize;
    if count == 0 || count > super::MAX_XDAT_ENTRIES {
        return Err(Malformed("implausible FSPX entry count"));
    }
    let end = 16 + 16 * count;
    part.get(..end)
        .ok_or(Malformed("FSPX table out of bounds"))?;
    let mut chunks = std::collections::BTreeMap::new();
    let mut ranges = Vec::new();
    for i in 0..count {
        let at = 16 + 16 * i;
        let kind = u32_at(part, at)?;
        let offset = u32_at(part, at + 4)? as usize;
        let len = u32_at(part, at + 8)? as usize;
        let stop = offset
            .checked_add(len)
            .ok_or(Malformed("FSPX size overflow"))?;
        if offset < end || stop > part.len() || ranges.iter().any(|&(a, b)| offset < b && a < stop)
        {
            return Err(Malformed("FSPX entry out of bounds or overlapping"));
        }
        if chunks.insert(kind, &part[offset..stop]).is_some() {
            return Err(Malformed("duplicate FSPX entry"));
        }
        ranges.push((offset, stop));
    }
    let chunk = |kind| {
        chunks
            .get(&kind)
            .copied()
            .ok_or(Malformed("FSPX entry missing"))
    };
    let packet = chunk(0)?;
    let (xy, z) = (u32_at(packet, 12)?, u32_at(packet, 16)?);
    let threads = [xy & 0xffff, xy >> 16, z & 0xffff];
    if threads.contains(&0)
        || threads
            .iter()
            .try_fold(1u32, |n, &v| n.checked_mul(v))
            .is_none_or(|n| n > 1024)
    {
        return Err(Malformed("invalid FSPX thread group"));
    }
    let wave64 = z >> 16 & 0x10 == 0;
    let (rsrc1, rsrc2) = (u32_at(packet, 20)?, u32_at(packet, 24)?);
    let vgprs = ((rsrc1 & 0x3f) + 1) * if wave64 { 4 } else { 8 };
    if vgprs > 256 {
        return Err(Malformed("implausible FSPX VGPR count"));
    }
    let code = chunk(6)?;
    let meta = chunk(7)?;
    if code.is_empty()
        || !code.len().is_multiple_of(4)
        || code.len() > 16 << 20
        || u32_at(meta, 8)? as usize != code.len() / 4
    {
        return Err(Malformed("FSPX code size disagrees"));
    }
    let entries = meta.get(24..).ok_or(Malformed("truncated FSPX metadata"))?;
    if !entries.len().is_multiple_of(32) {
        return Err(Malformed("truncated FSPX record"));
    }
    let mut used = Vec::new();
    let mut layout: Vec<RootParam> = Vec::new();
    for e in entries.chunks_exact(32) {
        match u32_at(e, 0)? {
            0x41 => {}
            0x42 => continue,
            0x43 => break,
            _ => return Err(Malformed("unknown FSPX metadata record")),
        }
        let root = RootType::from_raw(e[5].into());
        let u = UsedResource {
            kind: match e[4] {
                0 => ResourceType::Srv,
                1 => ResourceType::Sampler,
                2 => ResourceType::Cbv,
                3 => ResourceType::Uav,
                n => ResourceType::Other(n),
            },
            root,
            table_offset_dwords: u32_at(e, 8)?,
            lower_bound: u32_at(e, 12)?,
            // Field +16 is a resource dimension here, not a buffer stride.
            stride: 0,
            dwords: e[20],
            user_data: e[21],
            slot: e[22],
        };
        let sgprs = match root {
            RootType::Table => 1,
            RootType::Descriptor => 2,
            RootType::ImmediateSampler => 0,
            _ => return Err(Malformed("unknown FSPX resource root type")),
        };
        if sgprs > 0 {
            if u32::from(u.user_data) + sgprs > 102 {
                return Err(Malformed("FSPX root outside scalar registers"));
            }
            if let Some(p) = layout.iter().find(|p| p.slot == u.slot) {
                if (p.root, p.user_data, p.sgprs) != (root, u.user_data, sgprs) {
                    return Err(Malformed("inconsistent FSPX root metadata"));
                }
            } else {
                layout.push(RootParam {
                    root,
                    table_offset_dwords: 0,
                    sgprs,
                    base_register: u.lower_bound,
                    user_data: u.user_data,
                    slot: u.slot,
                });
            }
        }
        used.push(u);
    }
    Ok(NativeProgram {
        stage: HwStage::Cs,
        wave64,
        vgprs: vgprs as u16,
        layout,
        used,
        io: StageIo::Cs(ComputeIo { rsrc2, threads }),
        code: code.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut part = vec![0; 216];
        part[..4].copy_from_slice(b"FSPX");
        let words = [
            (4, 1),
            (8, 3),
            (12, 216),
            (16, 0),
            (20, 64),
            (24, 28),
            (32, 6),
            (36, 92),
            (40, 4),
            (48, 7),
            (52, 96),
            (56, 120),
            (76, 8 | 8 << 16),
            (80, 1 | 0x10 << 16),
            (84, 1),
            (92, 0xbf810000),
            (104, 1),
            (120, 0x41),
            (128, 16),
            (132, 2),
            (152, 0x42),
            (184, 0x43),
        ];
        for (at, word) in words {
            part[at..at + 4].copy_from_slice(&u32::to_le_bytes(word));
        }
        part[124] = 0;
        part[125] = 3;
        part[140] = 8;
        part[141] = 4;
        part[142] = 1;
        part
    }

    #[test]
    fn metadata_keeps_used_ranges_root_slots_group_size_and_wave32() {
        let p = program(&fixture()).unwrap();
        assert!(!p.wave64);
        assert_eq!(p.vgprs, 16);
        assert!(matches!(
            p.io,
            StageIo::Cs(ComputeIo {
                threads: [8, 8, 1],
                ..
            })
        ));
        assert_eq!(
            p.layout,
            vec![RootParam {
                root: RootType::Table,
                table_offset_dwords: 0,
                sgprs: 1,
                base_register: 2,
                user_data: 4,
                slot: 1
            }]
        );
        assert_eq!((p.used[0].table_offset_dwords, p.used[0].stride), (16, 0));
    }

    #[test]
    fn malformed_chunks_code_and_metadata_are_rejected() {
        let good = fixture();
        for (at, word) in [
            (4, 2),
            (8, 65),
            (12, 1000),
            (20, 0),
            (36, 64),
            (40, 3),
            (104, 2),
            (120, 0x99),
        ] {
            let mut bad = good.clone();
            bad[at..at + 4].copy_from_slice(&u32::to_le_bytes(word));
            assert!(program(&bad).is_err(), "offset {at}");
        }
        for end in 0..good.len() {
            assert!(program(&good[..end]).is_err());
        }
    }
}
