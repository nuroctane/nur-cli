use std::{env, fs, path::Path};

fn main() {
    if env::var_os("NUR_TEST_SCANNER_EMPTY").is_some() {
        return;
    }
    if env::var_os("NUR_TEST_SCANNER_FAIL").is_some() {
        println!("NEVER_SHOW_SECRET");
        std::process::exit(2);
    }
    if env::var_os("NUR_TEST_SCANNER_INVALID").is_some() {
        println!("{{invalid NEVER_SHOW_SECRET");
        return;
    }
    let args: Vec<String> = env::args().collect();
    let name = Path::new(&args[0]).file_stem().unwrap().to_string_lossy();
    if env::var_os("OPENAI_API_KEY").is_some() || (name == "trufflehog" && !args.iter().any(|arg| arg == "--no-verification")) {
        std::process::exit(3);
    }
    match name.as_ref() {
        "trivy" => {
            let output = args.iter().position(|arg| arg == "--output").unwrap();
            fs::write(&args[output + 1], r#"{"Results":[{"Target":"config.tf","Misconfigurations":[{"ID":"TRIVY-1","Title":"Unsafe setting","Severity":"HIGH","CauseMetadata":{"StartLine":3}}]}]}"#).unwrap();
        }
        "checkov" => println!("{{\"results\":{{\"failed_checks\":[{{\"check_id\":\"CKV-1\",\"check_name\":\"Bad setting\",\"file_path\":\"config.tf\",\"file_line_range\":[5,6],\"code_block\":\"NEVER_SHOW_SECRET\"}}]}}}}"),
        "trufflehog" => println!("{{\"DetectorName\":\"AWS\",\"Raw\":\"NEVER_SHOW_SECRET\",\"SourceMetadata\":{{\"Data\":{{\"Filesystem\":{{\"file\":\"config.tf\",\"line\":7}}}}}}}}"),
        _ => std::process::exit(1),
    }
}
