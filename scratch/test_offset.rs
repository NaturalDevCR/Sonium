use sonium_sync::TimeProvider;

fn main() {
    let tp = TimeProvider::new();
    // Case where server is 100ms ahead. RTT is 10ms.
    // T1 (client sent) = 0
    // T2 (server recv) = 105ms (5ms transit + 100ms offset)
    // T3 (client recv) = 10ms
    // Offset should be (T2 - (T1 + T3)/2) = 105 - 5 = 100ms.
    
    tp.update(0, 10_000, 105_000);
    let offset = tp.offset_us();
    println!("Offset: {}us (Expected 100000us)", offset);
    
    if offset == 100_000 {
        println!("SUCCESS");
    } else {
        println!("FAILURE");
    }
}
