//! Uses precisely the Tauri updater 2.11 signature verification primitive.
//! Upstream: plugins/updater/src/updater.rs::verify_signature (MIT/Apache-2.0).
use base64::Engine;
use minisign_verify::{PublicKey, Signature};
use std::{env, fs, process::ExitCode};

fn verify() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("expected PUBLIC_KEY_FILE DATA_FILE SIGNATURE_FILE".into());
    }
    let decode = |path: &std::ffi::OsStr| -> Result<String, Box<dyn std::error::Error>> {
        let encoded = fs::read_to_string(path)?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded.trim())?;
        Ok(String::from_utf8(bytes)?)
    };
    let public_key = PublicKey::decode(&decode(&args[0])?)?;
    let signature = Signature::decode(&decode(&args[2])?)?;
    public_key.verify(&fs::read(&args[1])?, &signature, true)?;
    Ok(())
}

fn main() -> ExitCode {
    match verify() {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => {
            // Never echo key material or signer diagnostics into release logs.
            eprintln!("Tauri updater signature verification failed");
            ExitCode::FAILURE
        }
    }
}
