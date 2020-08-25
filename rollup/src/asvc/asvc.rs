use ckb_zkp::scheme::asvc::{Parameters, Commitment, key_gen, commit, update_commit, prove_pos, Proof, UpdateKey, update_proof, verify_pos};
use ckb_zkp::scheme::r1cs::SynthesisError;
use ckb_zkp::math::{PairingEngine, fft::EvaluationDomain, ProjectiveCurve, Zero};
use rand::thread_rng;
use std::collections::HashMap;

pub fn initialize_asvc<E>(
    n: usize, 
) -> Result<(Parameters<E>, Commitment<E>, Vec<Proof<E>>), SynthesisError> 
where
    E: PairingEngine,
{
    let rng = &mut thread_rng();
    println!("start to initialize params...");
    if !n.is_power_of_two(){
        return Err(SynthesisError::Unsatisfiable);
    }
    let params = key_gen::<E, _>(n, rng)?;
    println!("initialize params...ok");

    println!("start to initialize commit...");
    let values = vec![E::Fr::zero(); n];
    let commit = commit::<E>(&params.proving_key, values)?;
    println!("initialize commit...ok");

    println!("start to initialize proofs...");
    let mut proofs = Vec::new();
    for i in 0..n {
        let proof = prove_pos::<E>(&params.proving_key, vec![E::Fr::zero()], vec![i as u32])?;
        proofs.push(proof);
    }
        
    println!("initialize proofs...ok");

    Ok((params, commit, proofs))
}

pub fn update_proofs<E>(
    upks: Vec<UpdateKey<E>>,
    commit: Commitment<E>,
    proofs: Vec<Proof<E>>,
    cvalues: &HashMap<u32, E::Fr>,
    n: usize,
) ->  Result<Vec<Proof<E>>, SynthesisError> 
where
    E: PairingEngine,
{
    let mut proofs = proofs;
    let domain = EvaluationDomain::<E::Fr>::new(n).ok_or(SynthesisError::PolynomialDegreeTooLarge)?;
    
    for (&j, &value) in cvalues {
        for i in 0..n {
            let proof = update_proof::<E>(&proofs[j as usize], value, i as u32, j, &upks[i as usize], &upks[j as usize], domain.group_gen, n)?; 
            proofs[i] = proof;
        }
    }

    Ok(proofs)
}