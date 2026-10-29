//! Instruction data: proof (A negated, B, C: 256 bytes) followed by the 13 public signals,
//! 32 bytes each, big-endian. Succeeds only if the proof verifies against the PARANOID key.
use groth16_solana::groth16::Groth16Verifier;
use solana_program::{account_info::AccountInfo, entrypoint, program_error::ProgramError, pubkey::Pubkey};

mod vk;

entrypoint!(process);

fn process(_: &Pubkey, _: &[AccountInfo], data: &[u8]) -> Result<(), ProgramError> {
    if data.len() != 256 + 32 * vk::NR_INPUTS {
        return Err(ProgramError::InvalidInstructionData);
    }
    let a: &[u8; 64] = data[0..64].try_into().unwrap();
    let b: &[u8; 128] = data[64..192].try_into().unwrap();
    let c: &[u8; 64] = data[192..256].try_into().unwrap();
    let mut inputs = [[0u8; 32]; vk::NR_INPUTS];
    for (i, chunk) in data[256..].chunks_exact(32).enumerate() {
        inputs[i].copy_from_slice(chunk);
    }
    Groth16Verifier::new(a, b, c, &inputs, &vk::VERIFYING_KEY)
        .and_then(|mut v| v.verify())
        .map_err(|_| ProgramError::Custom(1))
}
