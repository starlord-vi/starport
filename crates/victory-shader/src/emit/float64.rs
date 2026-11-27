// SPDX-License-Identifier: GPL-3.0-or-later
//! IEEE double operations; register pairs remain two independently guarded dwords.
use super::*;

impl Lower<'_> {
    pub(super) fn double_sum(
        &mut self,
        src: (Value, Value),
        mask: Value,
        high: bool,
    ) -> Result<V, Why> {
        if self.local || self.x.native.stage != HwStage::Cs {
            return Err("a double wave sum outside full compute waves");
        }
        if self
            .s
            .threads
            .is_none_or(|t| !t.iter().product::<u32>().is_multiple_of(64))
        {
            return Err("a double wave sum in a partial guest wave");
        }
        let halves = match (&self.exchange, self.s.wave_size) {
            (Some((_, false, _)), _) => true,
            (None, Some(64)) if self.x.p.wave64 => false,
            _ => return Err("a double wave sum on these waves"),
        };
        let every = if halves {
            self.all_lanes(self.cur_ir)
        } else {
            all_lane_blocks(self.x.st)
                .is_some_and(|all| all.get(self.cur_ir).copied().unwrap_or(false))
        };
        if !every {
            if halves {
                need(self.cur_ir);
            }
            return Err("a lane mask exchanged where not every lane runs");
        }
        let m = *self
            .x
            .st
            .masks
            .get(&mask)
            .ok_or("a double wave sum's untracked mask")?;
        let c = self.cur;
        let on = self.boolean(m, c)?;
        let zero = self.s.m.i32(0);
        let lo = self.i32_of(src.0)?;
        let hi = self.i32_of(src.1)?;
        let mut lo = self.s.m.select(c, on, lo, zero);
        let mut hi = self.s.m.select(c, on, hi, zero);
        let lane = self.s.op(
            c,
            "waveGetLaneIndex",
            Attr::ReadOnly,
            self.s.t.i32,
            111,
            &[],
        );
        self.s.flags |= flags::DOUBLE | flags::INT64 | flags::WAVE_OPS;
        for step in 0..if halves { 5 } else { 6 } {
            let bit = self.s.m.i32(1 << step);
            let partner = self.s.m.bin(c, Bin::Xor, lane, bit);
            let pl = self.read_lane(lo, partner);
            let ph = self.read_lane(hi, partner);
            (lo, hi) = self.add_double_words(lo, hi, pl, ph);
        }
        if halves {
            let (ll, hl) = self.exchange_halves(c, lo);
            let (lh, hh) = self.exchange_halves(c, hi);
            (lo, hi) = self.add_double_words(ll, lh, hl, hh);
        }
        Ok(if high { hi } else { lo })
    }

    fn add_double_words(&mut self, al: V, ah: V, bl: V, bh: V) -> (V, V) {
        let (a, b) = (self.join64(al, ah), self.join64(bl, bh));
        let ty = self.s.m.double();
        let c = self.cur;
        let a = self.s.m.cast(c, Cast::Bitcast, a, ty);
        let b = self.s.m.cast(c, Cast::Bitcast, b, ty);
        let sum = self.s.m.bin(c, Bin::Add, a, b);
        let int = self.s.m.int(64);
        let bits = self.s.m.cast(c, Cast::Bitcast, sum, int);
        self.split64(bits)
    }

    pub(super) fn valu64(&mut self, inst: &Inst, guard: Option<V>) -> Option<Result<(), Why>> {
        if !matches!(inst.name(), "v_add_f64" | "v_cvt_f32_f64") {
            return None;
        }
        Some(self.float64_op(inst, guard))
    }

    fn float64_op(&mut self, inst: &Inst, guard: Option<V>) -> Result<(), Why> {
        let Fields::Valu(v) = &inst.fields else {
            unreachable!()
        };
        if v.clamp || v.omod != 0 || v.sdwa.is_some() || v.dpp.is_some() || v.opsel != 0 {
            return Err("double-precision output modifiers or lane selectors");
        }
        self.s.flags |= flags::DOUBLE;
        let c = self.cur;
        let value = match inst.name() {
            "v_cvt_f32_f64" => {
                let input = self.float64_src(inst, 0)?;
                let result = self.s.m.cast(c, Cast::FpTrunc, input, self.s.t.f32);
                return self.write_f(inst, result, guard);
            }
            _ => {
                let a = self.float64_src(inst, 0)?;
                let b = self.float64_src(inst, 1)?;
                self.s.m.bin(c, Bin::Add, a, b)
            }
        };
        let Some(Operand::Vector { reg, .. }) = v.dst else {
            return Err("a double destination register pair");
        };
        let int = self.s.m.int(64);
        let bits = self.s.m.cast(c, Cast::Bitcast, value, int);
        let (lo, hi) = self.split64(bits);
        self.s.flags |= flags::INT64;
        self.write(inst, Reg::V(reg), lo, guard);
        self.write(
            inst,
            Reg::V(reg.checked_add(1).ok_or("a double destination pair")?),
            hi,
            guard,
        );
        Ok(())
    }

    fn float64_src(&mut self, inst: &Inst, k: usize) -> Result<V, Why> {
        let Fields::Valu(v) = &inst.fields else {
            unreachable!()
        };
        let operand = *v.src.get(k).ok_or("a missing double source")?;
        let pair = |this: &mut Self, lo: Reg, hi: Reg| -> Result<V, Why> {
            let lo = this.i32_of(inst.read(lo).ok_or("an unread double source")?)?;
            let hi = this.i32_of(inst.read(hi).ok_or("an unread double source")?)?;
            Ok(this.join64(lo, hi))
        };
        let int = self.s.m.int(64);
        let mut bits = match operand {
            Operand::Vector { reg, .. } => pair(
                self,
                Reg::V(reg),
                Reg::V(reg.checked_add(1).ok_or("a double source pair")?),
            )?,
            Operand::Scalar { code, .. } if code < 106 => {
                pair(self, Reg::S(code), Reg::S(code + 1))?
            }
            Operand::Const(code @ 240..=248) => {
                let value = crate::resource::inline(code).ok_or("a double inline constant")?;
                self.s
                    .m
                    .int_const(int, f64::from(f32::from_bits(value)).to_bits() as i64)
            }
            Operand::Const(128) | Operand::Scalar { code: NULL, .. } => self.s.m.int_const(int, 0),
            // Keep unverified literal/integer-constant encoding unsupported.
            _ => return Err("a double literal or special source"),
        };
        if v.abs >> k & 1 != 0 {
            let mask = self.s.m.int_const(int, i64::MAX);
            bits = self.s.m.bin(self.cur, Bin::And, bits, mask);
        }
        if v.neg >> k & 1 != 0 {
            let sign = self.s.m.int_const(int, i64::MIN);
            bits = self.s.m.bin(self.cur, Bin::Xor, bits, sign);
        }
        self.s.flags |= flags::INT64;
        let ty = self.s.m.double();
        Ok(self.s.m.cast(self.cur, Cast::Bitcast, bits, ty))
    }
}
