//! A tiny hand-written binary format: little-endian integers, floats as they are, short strings with a length byte.
//! Readers never panic and never allocate on the word of the sender: every count is checked against a limit first.

use crate::math::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireError {
    /// The message ended before everything it announced was there.
    Truncated,
    /// A value that cannot be valid (unknown tag, a count above its limit, a number out of range).
    Invalid(&'static str),
}

pub type Result<T> = std::result::Result<T, WireError>;

#[derive(Default, Debug)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self { buf: Vec::with_capacity(256) }
    }
    pub fn len(&self) -> usize {
        self.buf.len()
    }
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    pub fn bool(&mut self, v: bool) {
        self.buf.push(v as u8);
    }
    pub fn i8(&mut self, v: i8) {
        self.buf.push(v as u8);
    }
    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i16(&mut self, v: i16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn vec3(&mut self, v: Vec3) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z);
    }
    pub fn vec2(&mut self, v: Vec2) {
        self.f32(v.x);
        self.f32(v.y);
    }
    /// A string of at most 255 bytes (longer ones are cut at a character boundary).
    pub fn str(&mut self, s: &str) {
        let mut end = s.len().min(255);
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        self.u8(end as u8);
        self.buf.extend_from_slice(&s.as_bytes()[..end]);
    }
    /// An angle in radians, in 1/65536 of a turn.
    pub fn angle(&mut self, a: f32) {
        let t = (a / std::f32::consts::TAU).rem_euclid(1.0);
        self.u16((t * 65536.0) as u32 as u16);
    }
    /// A number in `0..=1` in 1/255 steps.
    pub fn unit(&mut self, v: f32) {
        self.u8((v.clamp(0.0, 1.0) * 255.0).round() as u8);
    }
    /// An optional small index: `None` is 255.
    pub fn opt_u8(&mut self, v: Option<usize>) {
        self.u8(v.map_or(255, |x| x.min(254) as u8));
    }
}

pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.remaining() < n {
            return Err(WireError::Truncated);
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn bool(&mut self) -> Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(WireError::Invalid("bool")),
        }
    }
    pub fn i8(&mut self) -> Result<i8> {
        Ok(self.u8()? as i8)
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    /// A float; NaN and infinities are refused (nothing in the game legitimately sends them).
    pub fn f32(&mut self) -> Result<f32> {
        let v = f32::from_le_bytes(self.take(4)?.try_into().unwrap());
        if v.is_finite() {
            Ok(v)
        } else {
            Err(WireError::Invalid("non-finite float"))
        }
    }
    pub fn vec3(&mut self) -> Result<Vec3> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
    pub fn vec2(&mut self) -> Result<Vec2> {
        Ok(Vec2::new(self.f32()?, self.f32()?))
    }
    pub fn str(&mut self) -> Result<String> {
        let n = self.u8()? as usize;
        let b = self.take(n)?;
        std::str::from_utf8(b).map(|s| s.to_string()).map_err(|_| WireError::Invalid("utf-8"))
    }
    pub fn angle(&mut self) -> Result<f32> {
        Ok(self.u16()? as f32 / 65536.0 * std::f32::consts::TAU)
    }
    pub fn unit(&mut self) -> Result<f32> {
        Ok(self.u8()? as f32 / 255.0)
    }
    pub fn opt_u8(&mut self) -> Result<Option<usize>> {
        Ok(match self.u8()? {
            255 => None,
            v => Some(v as usize),
        })
    }
    /// A count that must not exceed `max` (so a hostile length cannot make us reserve gigabytes).
    pub fn count(&mut self, max: usize, what: &'static str) -> Result<usize> {
        let n = self.u16()? as usize;
        if n > max {
            return Err(WireError::Invalid(what));
        }
        Ok(n)
    }
    /// An index into a table of `len` entries.
    pub fn index(&mut self, len: usize, what: &'static str) -> Result<usize> {
        let i = self.u8()? as usize;
        if i < len {
            Ok(i)
        } else {
            Err(WireError::Invalid(what))
        }
    }
    pub fn finished(&self) -> bool {
        self.remaining() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip() {
        let mut w = Writer::new();
        w.u8(200);
        w.bool(true);
        w.i8(-5);
        w.u16(65000);
        w.i16(-1234);
        w.u32(4_000_000_000);
        w.i32(-2_000_000_000);
        w.f32(-3.25);
        w.vec3(Vec3::new(1.0, -2.0, 3.5));
        w.vec2(Vec2::new(0.5, -0.5));
        w.str("héllo");
        w.angle(-1.0);
        w.unit(0.5);
        w.opt_u8(None);
        w.opt_u8(Some(7));
        let mut r = Reader::new(&w.buf);
        assert_eq!(r.u8(), Ok(200));
        assert_eq!(r.bool(), Ok(true));
        assert_eq!(r.i8(), Ok(-5));
        assert_eq!(r.u16(), Ok(65000));
        assert_eq!(r.i16(), Ok(-1234));
        assert_eq!(r.u32(), Ok(4_000_000_000));
        assert_eq!(r.i32(), Ok(-2_000_000_000));
        assert_eq!(r.f32(), Ok(-3.25));
        assert_eq!(r.vec3(), Ok(Vec3::new(1.0, -2.0, 3.5)));
        assert_eq!(r.vec2(), Ok(Vec2::new(0.5, -0.5)));
        assert_eq!(r.str().as_deref(), Ok("héllo"));
        let a = r.angle().unwrap();
        assert!((a - (std::f32::consts::TAU - 1.0)).abs() < 1e-3, "{a}");
        assert!((r.unit().unwrap() - 0.5).abs() < 0.003);
        assert_eq!(r.opt_u8(), Ok(None));
        assert_eq!(r.opt_u8(), Ok(Some(7)));
        assert!(r.finished());
    }

    #[test]
    fn angles_keep_their_direction_within_a_hundredth_of_a_degree() {
        for k in -40..40 {
            let a = k as f32 * 0.37;
            let mut w = Writer::new();
            w.angle(a);
            let b = Reader::new(&w.buf).angle().unwrap();
            let d = (a - b).rem_euclid(std::f32::consts::TAU);
            let d = d.min(std::f32::consts::TAU - d);
            assert!(d < 1.0e-4, "{a} -> {b}");
        }
    }

    #[test]
    fn long_strings_are_cut_on_a_character_boundary() {
        let s = "é".repeat(200); // 400 bytes
        let mut w = Writer::new();
        w.str(&s);
        let back = Reader::new(&w.buf).str().unwrap();
        assert_eq!(back.len(), 254);
        assert!(back.chars().all(|c| c == 'é'));
    }

    #[test]
    fn reading_past_the_end_or_bad_values_is_an_error_not_a_panic() {
        assert_eq!(Reader::new(&[]).u8(), Err(WireError::Truncated));
        assert_eq!(Reader::new(&[1, 2, 3]).u32(), Err(WireError::Truncated));
        assert_eq!(Reader::new(&[2]).bool(), Err(WireError::Invalid("bool")));
        assert!(Reader::new(&f32::NAN.to_le_bytes()).f32().is_err());
        assert!(Reader::new(&f32::INFINITY.to_le_bytes()).f32().is_err());
        assert!(Reader::new(&[5, b'a']).str().is_err()); // announces 5 bytes, has 1
        assert!(Reader::new(&[2, 0xff, 0xfe]).str().is_err()); // not UTF-8
        assert!(Reader::new(&[0xff, 0xff]).count(100, "n").is_err());
        assert!(Reader::new(&[9]).index(5, "i").is_err());
    }
}
