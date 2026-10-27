use super::*;

const HASH: [u8; 20] = [0x08, 0xad, 0xa5, 0xa7, 0xa6, 0x18, 0x3a, 0xae, 0x1e, 0x09, 0xd8, 0x31, 0xdf, 0x67, 0x48, 0xd5, 0x66, 0x09, 0x5a, 0x10];

#[test]
fn bencode_reads_what_trackers_send_and_nothing_malformed() {
    let v = bdecode(b"d3:bari-12e3:fool4:spami0eee").unwrap();
    assert_eq!(v.get(b"bar").and_then(Bencode::int), Some(-12));
    assert_eq!(v.get(b"foo"), Some(&Bencode::List(vec![Bencode::Bytes(b"spam".to_vec()), Bencode::Int(0)])));
    for bad in [&b""[..], b"i01e", b"i-0e", b"ie", b"i-e", b"5:abc", b"l", b"d3:fooe", b"i1ei2e", b"x", b"99999999999999999999:a"] {
        assert_eq!(bdecode(bad), None, "{:?}", String::from_utf8_lossy(bad));
    }
    // Nested too deep: refused, not a stack overflow.
    let deep = format!("{}{}", "l".repeat(10_000), "e".repeat(10_000));
    assert_eq!(bdecode(deep.as_bytes()), None);
}

#[test]
fn http_scrape_addresses_follow_bep_48() {
    let h = [b'a'; 20];
    let a = "a".repeat(20);
    assert_eq!(http_scrape_url("http://t.example/announce", &h), Some(format!("http://t.example/scrape?info_hash={a}")));
    assert_eq!(http_scrape_url("http://t.example/x/announce.php?k=1", &h), Some(format!("http://t.example/x/scrape.php?k=1&info_hash={a}")));
    assert_eq!(http_scrape_url("http://t.example/a", &h), None, "no announce in the last part: no scrape");
    assert_eq!(http_scrape_url("http://t.example/announce/x", &h), None);
    assert!(http_scrape_url("https://t.example/announce", &HASH).unwrap().ends_with("info_hash=%08%AD%A5%A7%A6%18%3A%AE%1E%09%D81%DFgH%D5f%09Z%10"));
}

#[test]
fn http_scrape_answers_are_read() {
    let mut body = b"d5:filesd20:".to_vec();
    body.extend_from_slice(&HASH);
    body.extend_from_slice(b"d8:completei368e10:downloadedi5347e10:incompletei2eeee");
    assert_eq!(parse_http_scrape(&body, &HASH), Ok(Swarm { seeds: 368, leechers: 2, completed: 5347 }));
    assert!(parse_http_scrape(&body, &[0; 20]).is_err(), "another torrent");
    assert_eq!(parse_http_scrape(b"d14:failure reason9:not founde", &HASH), Err("not found".into()));
    assert!(parse_http_scrape(b"<html>", &HASH).is_err());
}

#[test]
fn udp_scrapes_follow_bep_15() {
    let c = udp_connect(7);
    assert_eq!(&c[..8], &0x41727101980u64.to_be_bytes());
    assert_eq!(&c[8..12], &[0, 0, 0, 0]);
    assert_eq!(&c[12..], &7u32.to_be_bytes());
    let mut answer = vec![0, 0, 0, 0, 0, 0, 0, 7];
    answer.extend_from_slice(&0x1122334455667788u64.to_be_bytes());
    assert_eq!(udp_connected(&answer, 7), Ok(0x1122334455667788));
    assert!(udp_connected(&answer, 8).is_err(), "another transaction");
    assert!(udp_connected(&answer[..10], 7).is_err());

    let s = udp_scrape(0x1122334455667788, 9, &HASH);
    assert_eq!(&s[8..12], &2u32.to_be_bytes());
    assert_eq!(&s[16..], &HASH);
    let mut answer = vec![0, 0, 0, 2, 0, 0, 0, 9];
    for n in [368u32, 5347, 2] {
        answer.extend_from_slice(&n.to_be_bytes());
    }
    assert_eq!(udp_scraped(&answer, 9), Ok(Swarm { seeds: 368, completed: 5347, leechers: 2 }));
    let mut refused = vec![0, 0, 0, 3, 0, 0, 0, 9];
    refused.extend_from_slice(b"torrent not registered");
    assert_eq!(udp_scraped(&refused, 9), Err("torrent not registered".into()));
    assert!(udp_scraped(&answer[..12], 9).is_err());
}

#[test]
fn proxies_are_host_and_port_only() {
    assert_eq!(proxy_url(""), Ok(None));
    assert_eq!(proxy_url(" 127.0.0.1:9050 "), Ok(Some("socks5://127.0.0.1:9050".into())));
    assert_eq!(proxy_url("socks5://proxy.example:1080/"), Ok(Some("socks5://proxy.example:1080".into())));
    assert_eq!(proxy_url("[::1]:1080"), Ok(Some("socks5://[::1]:1080".into())));
    for bad in ["proxy.example", "proxy.example:0", "proxy.example:99999", "user:pass@proxy.example:1080", "a b:1", ":80"] {
        assert!(proxy_url(bad).is_err(), "{bad}");
    }
}

#[test]
fn ip_filters_in_both_forms_become_p2p() {
    let dat = "# eMule\n001.002.003.000 - 001.002.003.255 , 000 , Some range\n010.000.000.000 - 010.255.255.255 , 200 , allowed\nnonsense\n";
    assert_eq!(ip_filter_to_p2p(dat, 100), ("r0:1.2.3.0-1.2.3.255\n".into(), 1));
    let p2p = "Bad people:5.6.7.0-5.6.7.255\nv6:2001:db8::-2001:db8::ffff\nmixed:1.2.3.4-::1\n";
    assert_eq!(ip_filter_to_p2p(p2p, 100), ("r0:5.6.7.0-5.6.7.255\nr1:2001:db8::-2001:db8::ffff\n".into(), 2));
    assert_eq!(ip_filter_to_p2p(p2p, 1).1, 1, "capped");
}

#[test]
fn hex_hashes_read_back() {
    assert_eq!(hash_bytes("08ada5a7a6183aae1e09d831df6748d566095a10"), Some(HASH));
    assert_eq!(hash_bytes("08ada5"), None);
    assert_eq!(hash_bytes(&"zz".repeat(20)), None);
}
