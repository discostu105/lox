use lxir::XmlDocument;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: roundtrip_check <file.Loxone>");
    let input = std::fs::read(&path).unwrap();
    let doc = XmlDocument::parse(&input).unwrap();
    let output = doc.to_bytes();
    if input == output {
        println!("byte-identical roundtrip ({} bytes)", input.len());
    } else {
        println!(
            "DIFFERS: in {} bytes, out {} bytes",
            input.len(),
            output.len()
        );
        // locate first divergence
        let n = input
            .iter()
            .zip(&output)
            .take_while(|(a, b)| a == b)
            .count();
        println!("first divergence at byte {n}");
        let ctx = |b: &[u8]| {
            String::from_utf8_lossy(&b[n.saturating_sub(60)..(n + 60).min(b.len())]).to_string()
        };
        println!("input:  …{:?}…", ctx(&input));
        println!("output: …{:?}…", ctx(&output));
        std::process::exit(1);
    }
}
