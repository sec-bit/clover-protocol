use core::ops::{Add, AddAssign, Deref, Div, Mul, MulAssign, Neg, Sub, SubAssign};

use curve::{AffineCurve, ProjectiveCurve};
use math::fft::EvaluationDomain;
use math::fft::{DenseOrSparsePolynomial, DensePolynomial as Polynomial};
use math::fields::Field;
use math::msm::{FixedBaseMSM, VariableBaseMSM};
use math::{PairingEngine, PrimeField, UniformRand};
use num_traits::identities::{One, Zero};
use rand;
use rand::Rng;
use scheme::r1cs::SynthesisError;

pub struct UpdateKey<E: PairingEngine> {
    pub ai: E::G1Affine,
    pub ui: E::G1Affine,
}

pub struct ProvingKey<E: PairingEngine> {
    pub powers_of_g1: Vec<E::G1Affine>,
    pub l_of_g1: Vec<E::G1Affine>,
    pub update_keys: Vec<UpdateKey<E>>,
}

pub struct VerificationKey<E: PairingEngine> {
    pub powers_of_g1: Vec<E::G1Affine>,
    pub powers_of_g2: Vec<E::G2Affine>,
    pub a: E::G1Affine,
}

pub struct Parameters<E: PairingEngine> {
    pub proving_key: ProvingKey<E>,
    pub verification_key: VerificationKey<E>,
}

pub struct Commitment<E: PairingEngine> {
    pub commit: E::G1Affine,
}

pub struct Proof<E: PairingEngine> {
    pub w: E::G1Affine,
}

pub fn key_gen<E, R>(n: usize, rng: &mut R) -> Result<Parameters<E>, SynthesisError>
where
    E: PairingEngine,
    R: Rng,
{
    println!("[key_gen] start to setup...");
    let tau = E::Fr::rand(rng);
    let g1 = E::G1Projective::rand(rng);
    let g2 = E::G2Projective::rand(rng);
    println!("[key_gen] generate...ok.");

    let domain =
        EvaluationDomain::<E::Fr>::new(n).ok_or(SynthesisError::PolynomialDegreeTooLarge)?;
    let max_degree = domain.size();

    let scalar_bits = E::Fr::size_in_bits();
    let g1_window = FixedBaseMSM::get_mul_window_size(max_degree + 1);
    let g1_table = FixedBaseMSM::get_window_table::<E::G1Projective>(scalar_bits, g1_window, g1);

    let g2_window = FixedBaseMSM::get_mul_window_size(max_degree + 1);
    let g2_table = FixedBaseMSM::get_window_table::<E::G2Projective>(scalar_bits, g2_window, g2);
    println!("[key_gen] generate g1_table and g2_table...ok.");

    let mut curs = vec![E::Fr::one()];
    let mut cur = tau;
    for _ in 0..max_degree {
        curs.push(cur);
        cur.mul_assign(&tau);
    }
    let mut powers_of_g1 =
        FixedBaseMSM::multi_scalar_mul::<E::G1Projective>(scalar_bits, g1_window, &g1_table, &curs);
    let powers_of_g1 = E::G1Projective::batch_normalization_into_affine(&mut powers_of_g1);
    println!(
        "[key_gen] generate powers_of_g1...ok. max_degree = {}",
        max_degree
    );

    let mut powers_of_g2 =
        FixedBaseMSM::multi_scalar_mul::<E::G2Projective>(scalar_bits, g2_window, &g2_table, &curs);
    let powers_of_g2 = E::G2Projective::batch_normalization_into_affine(&mut powers_of_g2);
    println!(
        "[key_gen] generate powers_of_g2...ok. max_degree = {}",
        max_degree
    );

    // A(τ) = τ^n - 1
    let a = powers_of_g1[max_degree].into_projective().sub(&g1);
    println!("[key_gen] generate a=τ^n-1...ok.");

    println!("[key_gen] start generate i...");
    let mut update_keys: Vec<UpdateKey<E>> = Vec::new();
    let mut l_of_g1: Vec<E::G1Projective> = Vec::new();
    for i in 0..max_degree {
        // 1/(τ-ω^i)
        let tau_omega_i_divisor = E::Fr::one().div(&tau.sub(&domain.group_gen.pow(&[i as u64])));

        // ai = g_1^(A(τ)/(τ-ω^i))
        let ai = a.mul(tau_omega_i_divisor);

        // 1/nω^(n-i) = ω^i/n
        let a_aside_omega_i_divisor = domain
            .group_gen
            .pow(&[i as u64])
            .div(&E::Fr::from_repr((max_degree as u64).into()));

        // li = g_1^L_i(x) = g_1^(A(τ)/((x-ω^i)*A'(ω^i))) = ai^(1/A'(ω^i))
        let li = ai.mul(a_aside_omega_i_divisor);

        // ui = (li-1)/(x-ω^i)
        let mut ui = li.sub(&g1);
        ui = ui.mul(tau_omega_i_divisor);

        //batch_normalization_into_affine?
        let upk = UpdateKey {
            ai: ai.into_affine(),
            ui: ui.into_affine(),
        };
        update_keys.push(upk);
        l_of_g1.push(li);
    }

    let l_of_g1 = E::G1Projective::batch_normalization_into_affine(&mut l_of_g1);
    println!("[key_gen] generate i...ok");

    let params = Parameters::<E> {
        proving_key: ProvingKey::<E> {
            powers_of_g1: powers_of_g1.clone(),
            l_of_g1: l_of_g1,
            update_keys: update_keys,
        },
        verification_key: VerificationKey::<E> {
            powers_of_g1: powers_of_g1,
            powers_of_g2: powers_of_g2,
            a: a.into_affine(),
        },
    };
    Ok(params)
}

pub fn commit<E>(
    prk_params: &ProvingKey<E>,
    values: Vec<E::Fr>,
) -> Result<Commitment<E>, SynthesisError>
where
    E: PairingEngine,
{
    println!("[commit] start to commit...");
    let num_coefficient = values.len();
    let num_powers = prk_params.l_of_g1.len();

    println!(
        "num_coefficient = {}, num_powers = {}",
        num_coefficient, num_powers
    );
    assert!(num_coefficient >= 1);
    assert!(num_coefficient <= num_powers);

    println!("[commit] start generate commit...");
    let commit = VariableBaseMSM::multi_scalar_mul(
        &prk_params.l_of_g1.clone(),
        values
            .into_iter()
            .map(|e| e.into_repr())
            .collect::<Vec<_>>()
            .as_slice(),
    );
    println!("[commit] generate commit...ok.");
    // println!("[commit] generate commit...ok. commit = {}", commit);

    let c = Commitment::<E> {
        commit: commit.into_affine(),
    };
    println!("[commit]finish.");
    Ok(c)
}

pub fn prove_pos<E>(
    prk_params: &ProvingKey<E>,
    values: Vec<E::Fr>,
    points: Vec<u32>,
) -> Result<Proof<E>, SynthesisError>
where
    E: PairingEngine,
{
    let mut values = values.clone();
    let domain = EvaluationDomain::<E::Fr>::new(prk_params.powers_of_g1.len() - 1)
        .ok_or(SynthesisError::PolynomialDegreeTooLarge)?;
    domain.ifft_in_place(&mut values);
    let polynomial = Polynomial::from_coefficients_vec(values);

    // ∏(x-ω^i)
    let mut divisor_polynomial = Polynomial::from_coefficients_vec(vec![E::Fr::one()]);
    for point in points.iter() {
        let tpoly = Polynomial::from_coefficients_vec(vec![
            domain.group_gen.pow(&[*point as u64]).neg(),
            E::Fr::one(),
        ]);
        divisor_polynomial = divisor_polynomial.mul(&tpoly);
    }

    // Φ(x) / A_I(x) = q(x) ... r(x)
    let dense_or_sparse_poly: DenseOrSparsePolynomial<E::Fr> = polynomial.into();
    let dense_or_sparse_divisor: DenseOrSparsePolynomial<E::Fr> = divisor_polynomial.into();
    let (witness_polynomial, _) = dense_or_sparse_poly
        .divide_with_q_and_r(&dense_or_sparse_divisor)
        .unwrap();

    // π = g_1^q(τ)
    let witness = VariableBaseMSM::multi_scalar_mul(
        &prk_params.powers_of_g1.clone(),
        &witness_polynomial
            .deref()
            .to_vec()
            .into_iter()
            .map(|e| e.into_repr())
            .collect::<Vec<_>>(),
    );
    // println!("[open] evaluate the coeffieients for witness...OK. witness = {}", witness);

    let proof = Proof::<E> {
        w: witness.into_affine(),
    };

    Ok(proof)
}

pub fn verify_pos<E>(
    vrk_params: &VerificationKey<E>,
    commit: &Commitment<E>,
    point_values: Vec<E::Fr>,
    points: Vec<u32>,
    proof: &Proof<E>,
) -> Result<bool, SynthesisError>
where
    E: PairingEngine,
{
    println!("[verify] start to verify position...");
    let domain = EvaluationDomain::<E::Fr>::new(vrk_params.powers_of_g1.len() - 1)
        .ok_or(SynthesisError::PolynomialDegreeTooLarge)?;

    println!("[verify] start to evaluate lhs...");
    // A_I(x) = ∏(x - ω^i)
    let mut a_polynomial = Polynomial::from_coefficients_vec(vec![E::Fr::one()]);
    for point in points.iter() {
        let tpoly = Polynomial::from_coefficients_vec(vec![
            domain.group_gen.pow(&[*point as u64]).neg(),
            E::Fr::one(),
        ]);
        a_polynomial = a_polynomial.mul(&tpoly);
    }

    // r(x) = ∑（l_i * v_i） = ∑（A_I(x) * v_i）/(A_I'(ω^i)(x - ω_i))
    let mut r_polynomial = Polynomial::from_coefficients_vec(vec![E::Fr::zero()]);
    for (point, value) in points.iter().zip(point_values.iter()) {
        // x - ω_i
        let tpoly = Polynomial::from_coefficients_vec(vec![
            domain.group_gen.pow(&[*point as u64]).neg(),
            E::Fr::one(),
        ]);
        // A_I(x)/(x - ω_i)
        let mut l_polynomial = a_polynomial.div(&tpoly);
        // A_I'(ω^i)
        let b_aside = l_polynomial.evaluate(domain.group_gen.pow(&[*point as u64]));

        // v_i/A_I'(ω^i)
        let bpoly = Polynomial::from_coefficients_vec(vec![value.div(&b_aside)]);

        // (A_I(x) /(x - ω_i)) * (v_i/(A_I'(ω^i))
        l_polynomial = l_polynomial.mul(&bpoly);

        r_polynomial = r_polynomial.add(&l_polynomial);
    }
    let r_value = VariableBaseMSM::multi_scalar_mul(
        &vrk_params.powers_of_g1.clone(),
        &r_polynomial
            .deref()
            .into_iter()
            .map(|e| e.into_repr())
            .collect::<Vec<_>>(),
    );

    let mut inner = commit.commit.into_projective();
    inner.sub_assign(&r_value); // inner.sub_assign(&r_value);
    let lhs = E::pairing(inner, vrk_params.powers_of_g2[0]);
    println!("[verify] evaluate lhs...ok");

    println!("[verify] start to evaluate rhs...");
    // A_I(τ) = ∏(τ - ω^i)
    let a_value = VariableBaseMSM::multi_scalar_mul(
        &vrk_params.powers_of_g2.clone(),
        &a_polynomial
            .deref()
            .to_vec()
            .into_iter()
            .map(|e| e.into_repr())
            .collect::<Vec<_>>(),
    );
    let rhs = E::pairing(proof.w, a_value);
    println!("[verify] evaluate rhs...ok");

    println!("[verify] finish verify position...result = {}", lhs == rhs);
    Ok(lhs == rhs)
}

pub fn verify_upk<E>(
    vrk_params: &VerificationKey<E>,
    point: u32,
    upk: &UpdateKey<E>,
) -> Result<bool, SynthesisError>
where
    E: PairingEngine,
{
    // println!("[verify_upk] start to verify updating key...");
    // println!("[verify] start to verify e(a_i, g^i/g^(w^i)) = e(a,g)...");
    // println!("[verify] start to evaluate lhs...");
    // e(a_i, g^i/g^(w^i)) = e(a,g)
    let n = vrk_params.powers_of_g1.len() - 1;
    let domain =
        EvaluationDomain::<E::Fr>::new(n).ok_or(SynthesisError::PolynomialDegreeTooLarge)?;

    // ω^i
    let omega_i = domain.group_gen.pow(&[point as u64]);

    // g^τ / g^(ω^i)
    let inner = vrk_params.powers_of_g2[1]
        .into_projective()
        .sub(&vrk_params.powers_of_g2[0].into_projective().mul(omega_i));
    let lhs = E::pairing(upk.ai, inner);
    // println!("[verify_upk] evaluate lhs...ok");

    // println!("[verify_upk] start to evaluate rhs...");
    let rhs = E::pairing(vrk_params.a, vrk_params.powers_of_g2[0]);
    // println!("[verify_upk] evaluate rhs...ok");

    // e(a_i, g^τ / g^(ω^i)) = e(a, g), a_i^(τ - ω^i) = a
    let rs1 = lhs == rhs;
    // println!("[verify_upk] verify e(a_i, g^i/g^(w^i)) = e(a,g)...result = {}", rs1);

    // println!("[verify_upk] start to verify e(l_i/g, g) = e(u_i,g^τ/g^(ω^i))...");
    // println!("[verify_upk] start to evaluate lhs...");
    //a_i^(1/A'(ω^i))
    let a_aside_omega_i_divisor = domain
        .group_gen
        .pow(&[point as u64])
        .div(&E::Fr::from_repr((n as u64).into()));
    let l_value = upk.ai.mul(a_aside_omega_i_divisor);

    let inner2 = l_value.sub(&vrk_params.powers_of_g1[0].into_projective());
    let lhs = E::pairing(inner2, vrk_params.powers_of_g2[0]);
    // println!("[verify_upk] evaluate lhs...ok");

    // println!("[verify_upk] start to evaluate rhs...");
    let rhs = E::pairing(upk.ui, inner);
    // println!("[verify_upk] evaluate rhs...ok");
    let rs2 = lhs == rhs;
    // println!("[verify] verify e(l_i/g, g) = e(u_i,g^τ/g^(ω^i))...result = {}", rs2);

    Ok(rs1 && rs2)
}

pub fn update_commit<E>(
    commit: &Commitment<E>,
    delta: E::Fr,
    point: u32,
    upk: &UpdateKey<E>,
    n: usize,
) -> Result<Commitment<E>, SynthesisError>
where
    E: PairingEngine,
{
    println!("[update_commit] start to update commit...j = {}", point);
    let domain =
        EvaluationDomain::<E::Fr>::new(n).ok_or(SynthesisError::PolynomialDegreeTooLarge)?;
    let a_aside_omega_i_divisor = domain
        .group_gen
        .pow(&[point as u64])
        .div(&E::Fr::from_repr((n as u64).into()));
    let l_value = upk.ai.mul(a_aside_omega_i_divisor);

    let new_commit = commit.commit.into_projective().add(&(l_value.mul(delta)));
    let c = Commitment::<E> {
        commit: new_commit.into_affine(),
    };
    println!("[update_commit]finish.");
    Ok(c)
}

pub fn update_proof<E>(
    proof: &Proof<E>,
    delta: E::Fr,
    point_i: u32,
    point_j: u32,
    upk_i: &UpdateKey<E>,
    upk_j: &UpdateKey<E>,
    n: usize,
) -> Result<Proof<E>, SynthesisError>
where
    E: PairingEngine,
{
    let mut new_witness = proof.w.into_projective();
    // println!("[update_proof] start to update proof...i={}, j = {}", point_i, point_j);
    if point_i == point_j {
        new_witness.add_assign(&upk_i.ui.mul(delta));
    } else {
        //c_1 = 1/(ω_j - ω_i), c_2 = 1/(ω_i - ω_j)
        let domain =
            EvaluationDomain::<E::Fr>::new(n).ok_or(SynthesisError::PolynomialDegreeTooLarge)?;
        let omega_i = domain.group_gen.pow(&[point_i as u64]);
        let omega_j = domain.group_gen.pow(&[point_j as u64]);
        let c1 = E::Fr::one().div(&(omega_j.sub(&omega_i)));
        let c2 = E::Fr::one().div(&(omega_i.sub(&omega_j)));
        // w_ij = a_j^c_1 * a_i^c2
        let w_ij = upk_j.ai.mul(c1).add(&upk_i.ai.mul(c2));

        // u_ij = w_ij ^ (1/A'(w^j))
        let a_aside_omega_i_divisor = domain
            .group_gen
            .pow(&[point_j as u64])
            .div(&E::Fr::from_repr((n as u64).into()));
        let u_ij = w_ij.mul(a_aside_omega_i_divisor);
        new_witness.add_assign(&u_ij.mul(delta));
    }

    let proof = Proof::<E> {
        w: new_witness.into_affine(),
    };
    Ok(proof)
}

pub fn aggregate_proofs<E>(
    points: Vec<u32>,
    proofs: Vec<Proof<E>>,
    n: usize,
) -> Result<Proof<E>, SynthesisError>
where
    E: PairingEngine,
{
    let domain =
        EvaluationDomain::<E::Fr>::new(n).ok_or(SynthesisError::PolynomialDegreeTooLarge)?;
    // A(x) = ∏(x-ω^i)
    let mut a_polynomial = Polynomial::from_coefficients_vec(vec![E::Fr::one()]);
    for point in points.iter() {
        let tpoly = Polynomial::from_coefficients_vec(vec![
            domain.group_gen.pow(&[*point as u64]).neg(),
            E::Fr::one(),
        ]);
        a_polynomial = a_polynomial.mul(&tpoly);
    }

    let mut aggregate_witness = E::G1Projective::zero();
    for (point, proof) in points.iter().zip(proofs.iter()) {
        let divisor_polynomial = Polynomial::from_coefficients_vec(vec![
            domain.group_gen.pow(&[*point as u64]).neg(),
            E::Fr::one(),
        ]);
        let a_aside_polynomial = a_polynomial.div(&divisor_polynomial);
        let c =
            E::Fr::one().div(&a_aside_polynomial.evaluate(domain.group_gen.pow(&[*point as u64])));
        aggregate_witness.add_assign(&proof.w.mul(c));
    }

    let proof = Proof::<E> {
        w: aggregate_witness.into_affine(),
    };

    Ok(proof)
}

fn main() {
    use core::ops::Add;
    use curve::{bls12_381::Bls12_381, bn_256::Bn_256};
    use math::{PairingEngine, UniformRand};
    use rand::thread_rng;
    use std::time::Instant;

    fn aggregatable_svc_test() {
        let rng = &mut thread_rng();
        let size: usize = 8;
        let params = key_gen::<Bls12_381, _>(size, rng).unwrap();

        let mut values = Vec::<<Bls12_381 as PairingEngine>::Fr>::new();
        values.push(<Bls12_381 as PairingEngine>::Fr::rand(rng));
        values.push(<Bls12_381 as PairingEngine>::Fr::rand(rng));
        values.push(<Bls12_381 as PairingEngine>::Fr::rand(rng));
        values.push(<Bls12_381 as PairingEngine>::Fr::rand(rng));
        values.push(<Bls12_381 as PairingEngine>::Fr::rand(rng));
        values.push(<Bls12_381 as PairingEngine>::Fr::rand(rng));
        values.push(<Bls12_381 as PairingEngine>::Fr::rand(rng));
        values.push(<Bls12_381 as PairingEngine>::Fr::rand(rng));

        let c = commit(&params.proving_key, values.clone()).unwrap();

        println!("--------verify position...");
        let mut points = Vec::<u32>::new();
        let mut point_values = Vec::<<Bls12_381 as PairingEngine>::Fr>::new();
        points.push(0);
        point_values.push(values[0]);
        points.push(1);
        point_values.push(values[1]);
        points.push(5);
        point_values.push(values[5]);
        let proof = prove_pos(&params.proving_key, values.clone(), points.clone()).unwrap();
        let rs = verify_pos(&params.verification_key, &c, point_values, points, &proof).unwrap();
        println!("--------verify position...{}\n", rs);
        assert!(rs);

        println!("--------verify updating key...");
        let index: u32 = 2;
        let rs = verify_upk(
            &params.verification_key,
            index,
            &params.proving_key.update_keys[index as usize],
        )
        .unwrap();
        println!("--------verify updating key...{}\n", rs);
        assert!(rs);

        println!("--------verify update proof...");
        let index: u32 = 3;
        let delta = <Bls12_381 as PairingEngine>::Fr::rand(rng);
        let points_i = vec![index];
        let point_values_i = vec![values[index as usize].add(&delta)];
        let uc = update_commit(
            &c,
            delta,
            index,
            &params.proving_key.update_keys[index as usize],
            size,
        )
        .unwrap();
        let proof = prove_pos(&params.proving_key, values.clone(), points_i.clone()).unwrap();
        let proof = update_proof(
            &proof,
            delta,
            index,
            index,
            &params.proving_key.update_keys[index as usize],
            &params.proving_key.update_keys[index as usize],
            size,
        )
        .unwrap();
        let rs = verify_pos(
            &params.verification_key,
            &uc,
            point_values_i,
            points_i,
            &proof,
        )
        .unwrap();
        println!("--------verify update proof...{}\n", rs);
        assert!(rs);

        println!("--------start verify update proof, different index...");
        let index_i: u32 = 4;
        let points_i = vec![index_i];
        let point_values_i = vec![values[index_i as usize]];
        let proof = prove_pos(&params.proving_key, values.clone(), points_i.clone()).unwrap();
        let proof = update_proof(
            &proof,
            delta,
            index_i,
            index,
            &params.proving_key.update_keys[index_i as usize],
            &params.proving_key.update_keys[index as usize],
            size,
        )
        .unwrap();
        let rs = verify_pos(
            &params.verification_key,
            &uc,
            point_values_i,
            points_i,
            &proof,
        )
        .unwrap();
        println!("--------verify update proof, different index...{}\n", rs);
        assert!(rs);

        println!("--------start verify aggregate proofs...");
        let mut points = Vec::new();
        let mut point_values = Vec::new();
        let mut point_proofs = Vec::new();
        let point = vec![1];
        points.push(1);
        point_values.push(values[1]);
        let proof = prove_pos(&params.proving_key, values.clone(), point.clone()).unwrap();
        point_proofs.push(proof);

        let point = vec![5];
        points.push(5);
        point_values.push(values[5]);
        let proof = prove_pos(&params.proving_key, values.clone(), point.clone()).unwrap();
        point_proofs.push(proof);
        let proofs = aggregate_proofs(points.clone(), point_proofs, size).unwrap();
        let rs = verify_pos(&params.verification_key, &c, point_values, points, &proofs).unwrap();
        println!("--------verify aggregate proofs...{}\n", rs);
        assert!(rs);
    }

    fn aggregatable_svc_bench() {
        let size: usize = 2usize.pow(3);
        println!(
            "\n\n--------aggregatable svc test...curve = Bls12_381, size = {}",
            size
        );
        aggregatable_svc_bench_test::<Bls12_381>(size);

        let size: usize = 2usize.pow(6);
        println!(
            "\n\n--------aggregatable svc test...curve = Bls12_381, size = {}",
            size
        );
        aggregatable_svc_bench_test::<Bls12_381>(size);

        let size: usize = 2usize.pow(10);
        println!(
            "\n\n--------aggregatable svc test...curve = Bls12_381, size = {}",
            size
        );
        aggregatable_svc_bench_test::<Bls12_381>(size);

        let size: usize = 2usize.pow(14);
        println!(
            "\n\n--------aggregatable svc test...curve = Bls12_381, size = {}",
            size
        );
        aggregatable_svc_bench_test::<Bls12_381>(size);

        let size: usize = 2usize.pow(18);
        println!(
            "\n\n--------aggregatable svc test...curve = Bls12_381, size = {}",
            size
        );
        aggregatable_svc_bench_test::<Bls12_381>(size);

        let size: usize = 2usize.pow(21);
        println!(
            "\n\n--------aggregatable svc test...curve = Bls12_381, size = {}",
            size
        );
        aggregatable_svc_bench_test::<Bls12_381>(size);

        let size: usize = 2usize.pow(26);
        println!(
            "\n\n--------aggregatable svc test...curve = Bls12_381, size = {}",
            size
        );
        aggregatable_svc_bench_test::<Bls12_381>(size);

        let size: usize = 2usize.pow(3);
        println!(
            "\n\n--------aggregatable svc test...curve = Bn_256, size = {}",
            size
        );
        aggregatable_svc_bench_test::<Bn_256>(size);

        let size: usize = 2usize.pow(6);
        println!(
            "\n\n--------aggregatable svc test...curve = Bn_256, size = {}",
            size
        );
        aggregatable_svc_bench_test::<Bn_256>(size);

        let size: usize = 2usize.pow(10);
        println!(
            "\n\n--------aggregatable svc test...curve = Bn_256, size = {}",
            size
        );
        aggregatable_svc_bench_test::<Bn_256>(size);

        let size: usize = 2usize.pow(14);
        println!(
            "\n\n--------aggregatable svc test...curve = Bn_256, size = {}",
            size
        );
        aggregatable_svc_bench_test::<Bn_256>(size);

        let size: usize = 2usize.pow(18);
        println!(
            "\n\n--------aggregatable svc test...curve = Bn_256, size = {}",
            size
        );
        aggregatable_svc_bench_test::<Bn_256>(size);

        let size: usize = 2usize.pow(21);
        println!(
            "\n\n--------aggregatable svc test...curve = Bn_256, size = {}",
            size
        );
        aggregatable_svc_bench_test::<Bn_256>(size);

        let size: usize = 2usize.pow(26);
        println!(
            "\n\n--------aggregatable svc test...curve = Bn_256, size = {}",
            size
        );
        aggregatable_svc_bench_test::<Bn_256>(size);
    }

    fn aggregatable_svc_bench_test<E: PairingEngine>(size: usize) {
        let rng = &mut thread_rng();
        let start = Instant::now();
        let params = aggregatable_svc_key_gen_bench::<E, _>(size, rng).unwrap();
        println!(
            "aggregatable svc test, generate parameters...ok. size={}, time cost: {:?} ms",
            size,
            start.elapsed().as_millis(),
        );

        let start = Instant::now();
        let mut values = Vec::<E::Fr>::new();
        for _ in 0..size {
            values.push(E::Fr::rand(rng));
        }
        println!(
            "aggregatable svc test, generate values...ok. size={}, time cost: {:?} ms",
            size,
            start.elapsed().as_millis(),
        );

        let start = Instant::now();
        let commits = aggregatable_svc_commit_bench::<E>(&params, values.clone()).unwrap();
        println!(
            "aggregatable svc test, generate commitment...ok. size={}, time cost: {:?} ms",
            size,
            start.elapsed().as_millis(),
        );

        let mut points = Vec::new();
        for i in 0..size {
            points.push(i as u32);
        }
        let start = Instant::now();
        let proof =
            aggregatable_svc_prove_pos_bench::<E>(&params, points.clone(), values.clone()).unwrap();
        println!(
            "aggregatable svc test, generate proofs...ok. size={}, time cost: {:?} ms",
            size,
            start.elapsed().as_millis(),
        );

        let start = Instant::now();
        let rs = aggregatable_svc_verify_pos_bench::<E>(
            &params,
            &commits,
            points.clone(),
            values.clone(),
            &proof,
        )
        .unwrap();
        println!(
            "aggregatable svc test, verify proofs...ok. size={}, rs={}, time cost: {:?} ms",
            size,
            rs,
            start.elapsed().as_millis(),
        );

        let start = Instant::now();
        for i in 0..size {
            let rs = aggregatable_svc_verify_upk_bench::<E>(&params, i as u32).unwrap();
            if i % 1000 == 0 {
                println!(
                    "aggregatable svc test, verify verify upk...[{}]verify ok, time cost: {:?} ms",
                    i,
                    start.elapsed().as_millis(),
                );
            }
            assert!(rs);
        }
        println!(
            "aggregatable svc test, verify verify upk...ok. size={}, time cost: {:?} ms",
            size,
            start.elapsed().as_millis(),
        );

        let start = Instant::now();
        let mut proofs = Vec::<Proof<E>>::new();
        for i in 0..size {
            let mut point = Vec::<u32>::new();
            point.push(i as u32);
            let mut point_value = Vec::<E::Fr>::new();
            point_value.push(values[i]);
            let proof =
                aggregatable_svc_prove_pos_bench::<E>(&params, point.clone(), values.clone())
                    .unwrap();
            if i % 1000 == 0 {
                println!(
                    "aggregatable svc test, generate all the single proofs...ok. [{}] generate ok, time cost: {:?} ms",
                    i,
                    start.elapsed().as_millis(),
                );
            }

            proofs.push(proof);
        }
        println!(
            "aggregatable svc test, generate all the single proofs...ok. size={}, time cost: {:?} ms",
            size,
            start.elapsed().as_millis(),
        );

        let cindex = 3;
        let delta = E::Fr::rand(rng);
        let mut values = values.clone();
        values[cindex].add_assign(&delta);
        let start = Instant::now();
        let new_commits = aggregatable_svc_update_commit_bench::<E>(
            size,
            &params,
            &commits,
            cindex as u32,
            delta,
        )
        .unwrap();
        println!(
            "aggregatable svc test, change commitment...ok. size={}, time cost: {:?} ms",
            size,
            start.elapsed().as_millis(),
        );

        let start = Instant::now();
        let mut new_proofs = Vec::<Proof<E>>::new();
        for i in 0..size {
            let proof = aggregatable_svc_update_proof_bench::<E>(
                size,
                &params,
                &proofs[i],
                i as u32,
                cindex as u32,
                delta,
            )
            .unwrap();
            new_proofs.push(proof);
        }
        println!(
            "aggregatable svc test, update proof...ok. size={}, time cost: {:?} ms",
            size,
            start.elapsed().as_millis(),
        );

        let start = Instant::now();
        let asvc_proofs =
            aggregatable_svc_aggregate_proofs_bench::<E>(size, points.clone(), new_proofs).unwrap();
        println!(
            "aggregatable svc test, generate aggregatable svc proofs...ok. size={}, time cost: {:?} ms",
            size,
            start.elapsed().as_millis(),
        );

        let start = Instant::now();
        let rs = aggregatable_svc_verify_pos_bench::<E>(
            &params,
            &new_commits,
            points.clone(),
            values.clone(),
            &asvc_proofs,
        )
        .unwrap();
        println!(
            "aggregatable svc test, verify proofs...ok. size={}, rs={}, time cost: {:?} ms",
            size,
            rs,
            start.elapsed().as_millis(),
        );
    }

    fn aggregatable_svc_key_gen_bench<E: PairingEngine, R: rand::Rng>(
        size: usize,
        rng: &mut R,
    ) -> Result<Parameters<E>, SynthesisError> {
        let params = key_gen::<E, _>(size, rng);
        params
    }

    fn aggregatable_svc_commit_bench<E: PairingEngine>(
        params: &Parameters<E>,
        values: Vec<E::Fr>,
    ) -> Result<Commitment<E>, SynthesisError> {
        let commits = commit(&params.proving_key, values.clone()).unwrap();
        Ok(commits)
    }

    fn aggregatable_svc_prove_pos_bench<E: PairingEngine>(
        params: &Parameters<E>,
        points: Vec<u32>,
        values: Vec<E::Fr>,
    ) -> Result<Proof<E>, SynthesisError> {
        let proof = prove_pos(&params.proving_key, values.clone(), points.clone());
        proof
    }

    fn aggregatable_svc_verify_pos_bench<E: PairingEngine>(
        params: &Parameters<E>,
        commits: &Commitment<E>,
        points: Vec<u32>,
        values: Vec<E::Fr>,
        proof: &Proof<E>,
    ) -> Result<bool, SynthesisError> {
        let rs = verify_pos(&params.verification_key, &commits, values, points, proof);
        rs
    }

    fn aggregatable_svc_verify_upk_bench<E: PairingEngine>(
        params: &Parameters<E>,
        index: u32,
    ) -> Result<bool, SynthesisError> {
        let rs = verify_upk(
            &params.verification_key,
            index,
            &params.proving_key.update_keys[index as usize],
        );
        rs
    }

    fn aggregatable_svc_update_commit_bench<E: PairingEngine>(
        size: usize,
        params: &Parameters<E>,
        commits: &Commitment<E>,
        index: u32,
        delta: E::Fr,
    ) -> Result<Commitment<E>, SynthesisError> {
        let uc = update_commit(
            &commits,
            delta,
            index,
            &params.proving_key.update_keys[index as usize],
            size,
        );
        uc
    }

    fn aggregatable_svc_update_proof_bench<E: PairingEngine>(
        size: usize,
        params: &Parameters<E>,
        proof: &Proof<E>,
        index: u32,
        cindex: u32,
        delta: E::Fr,
    ) -> Result<Proof<E>, SynthesisError> {
        let proof = update_proof(
            &proof,
            delta,
            index,
            cindex,
            &params.proving_key.update_keys[index as usize],
            &params.proving_key.update_keys[cindex as usize],
            size,
        );
        proof
    }

    fn aggregatable_svc_aggregate_proofs_bench<E: PairingEngine>(
        size: usize,
        points: Vec<u32>,
        point_proofs: Vec<Proof<E>>,
    ) -> Result<Proof<E>, SynthesisError> {
        let proofs = aggregate_proofs(points, point_proofs, size);
        proofs
    }

    aggregatable_svc_test();
    aggregatable_svc_bench();
}
