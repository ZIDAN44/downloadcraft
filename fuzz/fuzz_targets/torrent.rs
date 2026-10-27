//! What trackers and files from elsewhere hand the torrent side: bencoded scrape answers (BEP 48),
//! UDP tracker answers (BEP 15), IP filter files, proxy addresses and info hashes typed or pasted.
#![no_main]

use downloadcraft_model::torrent::{bdecode, hash_bytes, http_scrape_url, ip_filter_to_p2p, parse_http_scrape, proxy_url, udp_connected, udp_scraped};
use libfuzzer_sys::fuzz_target;

const HASH: [u8; 20] = [7; 20];

fuzz_target!(|data: &[u8]| {
    let _ = bdecode(data);
    let _ = parse_http_scrape(data, &HASH);
    let _ = udp_connected(data, 7);
    let _ = udp_scraped(data, 7);
    let Ok(text) = std::str::from_utf8(data) else { return };
    // Every line written out is a range of the P2P form, at most as many as asked.
    let (p2p, n) = ip_filter_to_p2p(text, 1000);
    assert!(n <= 1000);
    assert_eq!(p2p.lines().count(), n);
    for line in p2p.lines() {
        let (_, range) = line.split_once(':').unwrap_or_default();
        assert!(range.contains('-'), "{line}");
    }
    // A proxy that's taken is written back as socks5://host:port, and reads the same again.
    if let Ok(Some(url)) = proxy_url(text) {
        assert!(url.starts_with("socks5://"), "{url}");
        assert_eq!(proxy_url(&url), Ok(Some(url.clone())));
    }
    let _ = hash_bytes(text);
    if let Some(url) = http_scrape_url(text, &HASH) {
        assert!(url.contains("info_hash="), "{url}");
    }
});
