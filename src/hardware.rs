use alloy_consensus::SignableTransaction;
use alloy_primitives::{B256, Signature};
use alloy_signer::Signer;
use anyhow::{anyhow, Context, Result};

/// Wraps either a Ledger or Trezor hardware signer.
pub enum HardwareDevice {
    Ledger(alloy_signer_ledger::LedgerSigner),
    Trezor(alloy_signer_trezor::TrezorSigner),
}

// HardwareDevice is not Send because the underlying HID transports aren't,
// but we only ever access it from spawned threads with their own tokio runtime.
unsafe impl Send for HardwareDevice {}
unsafe impl Sync for HardwareDevice {}

impl std::fmt::Debug for HardwareDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HardwareDevice::Ledger(_) => f.write_str("HardwareDevice::Ledger"),
            HardwareDevice::Trezor(_) => f.write_str("HardwareDevice::Trezor"),
        }
    }
}

/// Try to connect to a Ledger first, then Trezor. Returns whichever succeeds.
pub async fn detect_and_connect(chain_id: u64) -> Result<HardwareDevice> {
    // Try Ledger first
    match alloy_signer_ledger::LedgerSigner::new(
        alloy_signer_ledger::HDPath::LedgerLive(0),
        Some(chain_id),
    )
    .await
    {
        Ok(signer) => {
            eprintln!("[hardware] Ledger detected, address: 0x{:x}", signer.address());
            return Ok(HardwareDevice::Ledger(signer));
        }
        Err(e) => {
            eprintln!("[hardware] Ledger detection failed: {e}");
        }
    }

    // Try Trezor
    match alloy_signer_trezor::TrezorSigner::new(
        alloy_signer_trezor::HDPath::TrezorLive(0),
        Some(chain_id),
    )
    .await
    {
        Ok(signer) => {
            eprintln!("[hardware] Trezor detected, address: 0x{:x}", signer.address());
            return Ok(HardwareDevice::Trezor(signer));
        }
        Err(e) => {
            eprintln!("[hardware] Trezor detection failed: {e}");
        }
    }

    Err(anyhow!(
        "No hardware wallet detected. Please plug in your Ledger or Trezor and unlock it."
    ))
}

/// Get the address from a hardware device.
pub fn get_address(device: &HardwareDevice) -> String {
    match device {
        HardwareDevice::Ledger(s) => format!("0x{:x}", s.address()),
        HardwareDevice::Trezor(s) => format!("0x{:x}", s.address()),
    }
}

/// Sign a personal message (EIP-191).
pub async fn sign_message(device: &HardwareDevice, msg: &[u8]) -> Result<String> {
    let sig = match device {
        HardwareDevice::Ledger(s) => s
            .sign_message(msg)
            .await
            .context("Ledger sign_message failed")?,
        HardwareDevice::Trezor(s) => s
            .sign_message(msg)
            .await
            .context("Trezor sign_message failed")?,
    };
    Ok(format!("0x{}", hex::encode(sig.as_bytes())))
}

/// Sign a hash (used for typed data fallback).
pub async fn sign_hash(device: &HardwareDevice, hash: B256) -> Result<String> {
    // Hardware wallets generally don't support raw hash signing.
    // For EIP-712 typed data, the device needs the structured data.
    // As a fallback we sign the hash as a message.
    let sig = match device {
        HardwareDevice::Ledger(s) => s
            .sign_message(hash.as_slice())
            .await
            .context("Ledger sign_hash failed")?,
        HardwareDevice::Trezor(s) => s
            .sign_message(hash.as_slice())
            .await
            .context("Trezor sign_hash failed")?,
    };
    Ok(format!("0x{}", hex::encode(sig.as_bytes())))
}

/// Sign a transaction and return the hex-encoded signature.
pub async fn sign_transaction(
    device: &HardwareDevice,
    tx: &mut dyn SignableTransaction<Signature>,
) -> Result<Signature> {
    let sig = match device {
        HardwareDevice::Ledger(s) => alloy_network::TxSigner::sign_transaction(s, tx)
            .await
            .context("Ledger sign_transaction failed")?,
        HardwareDevice::Trezor(s) => alloy_network::TxSigner::sign_transaction(s, tx)
            .await
            .context("Trezor sign_transaction failed")?,
    };
    Ok(sig)
}
