// SPDX-License-Identifier: GPL-3.0-or-later
// Focused ACCC operations adapted from codec-d--cool_fox-OPTIMIZED:
// https://git.gtahub.st/Mskh/codec-d--cool_fox-OPTIMIZED
// 808342e93a268ded13e67f4aca016214c6d3230d (inherited contributors).
//! Native upscaler operations sharing the checked emitter state.
use super::*;

impl Lower<'_> {
    pub(super) fn accc_dot(&mut self, inst: &Inst, guard: Option<V>) -> Result<(), Why> {
        let Fields::Valu(v) = &inst.fields else {
            unreachable!()
        };
        let name = inst.name();
        let c = self.cur;
        let s = |l: &mut Self, k| l.src(inst, k);
        let int = |l: &mut Self, r| l.write_i(inst, r, guard);
        match name {
            "v_dot4c_i32_i8" => {
                // Signed packed bytes, accumulating with i32 wrapping. From
                // codec-d 808342e93a268ded13e67f4aca016214c6d3230d.
                let (a, bb) = (s(self, 0)?, s(self, 1)?);
                let Some(Operand::Vector { reg, .. }) = v.dst else {
                    return Err("an accumulator");
                };
                let acc = self.i32_of(inst.read(Reg::V(reg)).ok_or("an accumulator")?)?;
                let i32_ty = self.s.t.i32;
                let r = self.s.op(
                    c,
                    "dot4AddPacked.i32",
                    Attr::ReadNone,
                    i32_ty,
                    163,
                    &[acc, a, bb],
                );
                int(self, r)
            }
            "v_dot4_i32_i8" => {
                // VOP3P half selectors build the two packed multiplicands;
                // the accumulator is a complete wrapping i32 value.
                if v.clamp || v.neg_hi != 0 {
                    return Err("a saturating or negated dot product");
                }
                let mut xs = Vec::new();
                for k in 0..2 {
                    let x = s(self, k)?;
                    let (lo, hi) = (v.opsel >> k & 1 != 0, v.opsel_hi >> k & 1 != 0);
                    if (lo, hi) == (false, true) {
                        xs.push(x);
                        continue;
                    }
                    let (k16, low) = (self.s.m.i32(16), self.s.m.i32(0xFFFF));
                    let top = self.s.m.bin(c, Bin::LShr, x, k16);
                    let bottom = self.s.m.bin(c, Bin::And, x, low);
                    let hi = self
                        .s
                        .m
                        .bin(c, Bin::Shl, if hi { top } else { bottom }, k16);
                    xs.push(self.s.m.bin(c, Bin::Or, if lo { top } else { bottom }, hi));
                }
                let acc = s(self, 2)?;
                let i32_ty = self.s.t.i32;
                let r = self.s.op(
                    c,
                    "dot4AddPacked.i32",
                    Attr::ReadNone,
                    i32_ty,
                    163,
                    &[acc, xs[0], xs[1]],
                );
                int(self, r)
            }
            _ => unreachable!(),
        }
    }
    pub(super) fn permlane(&mut self, inst: &Inst, guard: Option<V>) -> Result<(), Why> {
        let Fields::Valu(v) = &inst.fields else {
            unreachable!()
        };
        let name = inst.name();
        let c = self.cur;
        let s = |l: &mut Self, k| l.src(inst, k);
        let int = |l: &mut Self, r| l.write_i(inst, r, guard);
        match name {
            "v_permlane16_b32" | "v_permlanex16_b32" => {
                // Each 4-bit selector picks within a 16-lane row; the x
                // variant swaps rows within the same 32-lane half. Adapted
                // from codec-d 808342e93a268ded13e67f4aca016214c6d3230d.
                if v.abs != 0 || v.neg != 0 || v.opsel & !3 != 0 {
                    return Err("permlane input modifiers");
                }
                self.cross_lane(false)?;
                let fetch_inactive = v.opsel & 1 != 0;
                if fetch_inactive && !all_lane_blocks(self.x.st).is_some_and(|all| all[self.cur_ir])
                {
                    return Err("a permlane fetch from lanes outside the host branch");
                }
                let (x, lo, hi) = (s(self, 0)?, s(self, 1)?, s(self, 2)?);
                let lane = self.lane_index();
                let (k7, k8, k15, k16, k31, k32, k4, one, zero) = (
                    self.s.m.i32(7),
                    self.s.m.i32(8),
                    self.s.m.i32(15),
                    self.s.m.i32(16),
                    self.s.m.i32(31),
                    self.s.m.i32(32),
                    self.s.m.i32(4),
                    self.s.m.i32(1),
                    self.s.m.i32(0),
                );
                let pos = self.s.m.bin(c, Bin::And, lane, k15);
                let lower = self.s.m.cmp(c, pred::IULT, pos, k8);
                let packed = self.s.m.select(c, lower, lo, hi);
                let shift = self.s.m.bin(c, Bin::And, pos, k7);
                let shift = self.s.m.bin(c, Bin::Mul, shift, k4);
                let pick = self.s.m.bin(c, Bin::LShr, packed, shift);
                let pick = self.s.m.bin(c, Bin::And, pick, k15);
                let not15 = self.s.m.i32(!15);
                let mut row = self.s.m.bin(c, Bin::And, lane, not15);
                if name == "v_permlanex16_b32" {
                    row = self.s.m.bin(c, Bin::Xor, row, k16);
                }
                let src = self.s.m.bin(c, Bin::Or, row, pick);
                let r = if fetch_inactive {
                    self.read_lane(x, src)
                } else {
                    let enabled = guard.unwrap_or_else(|| self.s.m.bool(true));
                    let i32t = self.s.t.i32;
                    let four = self.s.m.named_struct("dx.types.fouri32", vec![i32t; 4]);
                    let mask =
                        self.s
                            .op(c, "waveActiveBallot", Attr::NoUnwind, four, 116, &[enabled]);
                    let lo = self.s.m.extract(c, mask, 0);
                    let hi = self.s.m.extract(c, mask, 1);
                    let lower = self.s.m.cmp(c, pred::IULT, src, k32);
                    let word = self.s.m.select(c, lower, lo, hi);
                    let bit = self.s.m.bin(c, Bin::And, src, k31);
                    let bit = self.s.m.bin(c, Bin::LShr, word, bit);
                    let bit = self.s.m.bin(c, Bin::And, bit, one);
                    let active = self.s.m.cmp(c, pred::NE, bit, zero);
                    let safe = self.s.m.select(c, active, src, lane);
                    let read = self.read_lane(x, safe);
                    let fallback = if v.opsel & 2 != 0 {
                        zero
                    } else {
                        let Some(Operand::Vector { reg, .. }) = v.dst else {
                            return Err("a permlane result outside a VGPR");
                        };
                        self.i32_of(inst.read(Reg::V(reg)).ok_or("a permlane old value")?)?
                    };
                    self.s.m.select(c, active, read, fallback)
                };
                int(self, r)
            }
            _ => unreachable!(),
        }
    }
    /// VOP2 packed FMAC reads both halves and its destination accumulator.
    pub(super) fn fmac_half(&mut self, inst: &Inst, k: usize, high: bool) -> Result<V, Why> {
        let Fields::Valu(v) = &inst.fields else {
            unreachable!()
        };
        let op = if k == 2 { v.dst } else { v.src.get(k).copied() };
        let op = op.ok_or("a missing operand")?;
        if matches!(op, Operand::Const(_) | Operand::Literal(_)) {
            return Err("a constant operand of v_pk_fmac_f16");
        }
        let x = self.operand(inst, &op)?;
        let c = self.cur;
        let x = if high {
            let k16 = self.s.m.i32(16);
            self.s.m.bin(c, Bin::LShr, x, k16)
        } else {
            x
        };
        let m = self.s.m.i32(0xFFFF);
        Ok(self.s.m.bin(c, Bin::And, x, m))
    }

    /// A one-channel integer image, proved by an atomic on the same entry.
    pub(super) fn is_counter(&self, src: &Source) -> bool {
        let Some(this) = self.entry(src) else {
            return false;
        };
        self.x.res.descriptors.iter().any(|d| {
            let i = &self.x.p.blocks[d.site.0].insts[d.site.1];
            d.role == Role::Resource
                && matches!(i.fields, Fields::Image { dmask: 1, .. })
                && matches!(
                    i.name(),
                    "image_atomic_add"
                        | "image_atomic_sub"
                        | "image_atomic_and"
                        | "image_atomic_or"
                        | "image_atomic_xor"
                        | "image_atomic_smin"
                        | "image_atomic_smax"
                        | "image_atomic_umin"
                        | "image_atomic_umax"
                )
                && self.entry(&d.source) == Some(this)
        })
    }

    fn entry(&self, src: &Source) -> Option<(usize, u32)> {
        let Source::Loaded { load, first } = src else {
            return None;
        };
        let a = self.scalar.get(load).filter(|a| a.dynamic.is_none())?;
        Some((a.param, a.offset + 4 * u32::from(*first)))
    }
}
