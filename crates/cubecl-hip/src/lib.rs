#[allow(unused_imports)]
#[macro_use]
extern crate derive_new;
extern crate alloc;

#[cfg(target_os = "linux")]
pub mod compute;
#[cfg(target_os = "linux")]
pub mod device;
#[cfg(target_os = "linux")]
pub mod runtime;
#[cfg(target_os = "linux")]
pub use device::*;
#[cfg(target_os = "linux")]
pub use runtime::HipRuntime;
#[cfg(feature = "rocwmma")]
pub(crate) type HipWmmaCompiler = cubecl_cpp::hip::wmma::RocWmmaCompiler;
#[cfg(not(feature = "rocwmma"))]
pub(crate) type HipWmmaCompiler = cubecl_cpp::hip::wmma::WmmaIntrinsicCompiler;

#[cfg(target_os = "linux")]
#[cfg(test)]
mod tests {
    pub type TestRuntime = crate::HipRuntime;

    cubecl_core::testgen_all!();
    cubecl_linalg::testgen_cmma_matmul!();
}
