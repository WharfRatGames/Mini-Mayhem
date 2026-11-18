#[test]
fn dlog_appends_lines_after_start() {
    let p = std::env::temp_dir().join(format!("arty-dlog-test-{}.log", std::process::id()));
    std::env::set_var("ARTY_LOG", &p);
    arty::dlog::init("test");
    arty::dlog!("net", "connect port={} ok={}", 7787, true);
    let mut w = arty::dlog::PerfWin::new();
    for _ in 0..40 { w.frame(33.0, 4.0, 1, Some(10.0), 20); std::thread::sleep(std::time::Duration::from_millis(30)); }
    let s = std::fs::read_to_string(&p).unwrap();
    println!("{s}");
    assert!(s.contains("[net]"), "{s}");
    assert!(s.contains("[perf]"), "{s}");
}
