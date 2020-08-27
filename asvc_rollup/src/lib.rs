#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]
use alloc::{format, string::String, vec::Vec};

#[cfg(feature = "std")]
use std::{format, string::String, vec::Vec};

pub mod block;
pub mod transaction;
