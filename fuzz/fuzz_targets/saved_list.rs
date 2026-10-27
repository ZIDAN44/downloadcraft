//! `downloads.json` as it may be found: damaged, hand-edited or from another version. Loading it is
//! an error or a list, and the list saves and loads again the same.
#![no_main]

use downloadcraft_engine::Session;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(data) else { return };
    let mut s = Session::new();
    if s.load(v).is_ok() {
        let saved = s.save();
        let mut again = Session::new();
        again.load(saved.clone()).expect("a list it saved loads again");
        assert_eq!(again.save(), saved, "and saves the same");
    }
});
