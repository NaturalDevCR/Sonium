struct SampleBuffer {
    buf: [i64; 200],
    len: usize,
    pos: usize,
}
impl SampleBuffer {
    fn new() -> Self { Self { buf: [0i64; 200], len: 0, pos: 0 } }
    fn push(&mut self, v: i64) {
        self.buf[self.pos] = v;
        self.pos = (self.pos + 1) % 200;
        if self.len < 200 { self.len += 1; }
    }
    fn median(&self) -> i64 {
        if self.len == 0 { return 0; }
        let mut sorted: Vec<i64> = self.buf[..self.len].to_vec();
        sorted.sort_unstable();
        sorted[self.len / 2]
    }
}
fn main() {
    let mut b = SampleBuffer::new();
    for i in 1..=5 { b.push(i); }
    println!("len={}, pos={}, median={}", b.len, b.pos, b.median());
    for i in 6..=205 { b.push(i); }
    println!("len={}, pos={}, median={}", b.len, b.pos, b.median());
}
