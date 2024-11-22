use std::{marker::PhantomData, num::NonZero};

use cubecl_runtime::{server::ComputeServer, storage::ComputeStorage};

use crate::{
    compute::{KernelBuilder, KernelLauncher},
    ir::{Item, Vectorization},
    prelude::{
        ArgSettings, CubePrimitive, ExpandElementTyped, LaunchArg, LaunchArgExpand, TensorHandleRef,
    },
    Runtime,
};

use super::Array;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ArrayCompilationArg {
    pub inplace: Option<u16>,
    pub vectorisation: Vectorization,
}

/// Tensor representation with a reference to the [server handle](cubecl_runtime::server::Handle).
pub struct ArrayHandleRef<'a, R: Runtime> {
    pub handle: &'a cubecl_runtime::server::Handle,
    pub(crate) length: [usize; 1],
    pub elem_size: usize,
    runtime: PhantomData<R>,
}

impl<C: CubePrimitive> LaunchArgExpand for Array<C> {
    type CompilationArg = ArrayCompilationArg;

    fn expand(
        arg: &Self::CompilationArg,
        builder: &mut KernelBuilder,
    ) -> ExpandElementTyped<Array<C>> {
        builder
            .input_array(Item::vectorized(C::as_elem(), arg.vectorisation))
            .into()
    }
    fn expand_output(
        arg: &Self::CompilationArg,
        builder: &mut KernelBuilder,
    ) -> ExpandElementTyped<Array<C>> {
        match arg.inplace {
            Some(id) => builder.inplace_output(id).into(),
            None => builder
                .output_array(Item::vectorized(C::as_elem(), arg.vectorisation))
                .into(),
        }
    }
}

struct RawResource<S: ComputeStorage>(S::Resource);

unsafe impl<S: ComputeStorage> Send for RawResource<S> {}
unsafe impl<S: ComputeStorage> Sync for RawResource<S> {}

pub enum ArrayArg<'a, R: Runtime> {
    /// The array is passed with an array handle.
    Handle {
        /// The array handle.
        handle: ArrayHandleRef<'a, R>,
        /// The vectorization factor.
        vectorization_factor: u8,
    },
    /// The array is aliasing another input array.
    Alias {
        /// The position of the input array.
        input_pos: usize,
    },
    /// The
    Resource(RawResource<<R::Server as ComputeServer>::Storage>),
}

impl<'a, R: Runtime> ArgSettings<R> for ArrayArg<'a, R> {
    fn register(&self, launcher: &mut KernelLauncher<R>) {
        launcher.register_array(self)
    }
}

impl<'a, R: Runtime> ArrayArg<'a, R> {
    /// Create a new array argument.
    ///
    /// # Safety
    ///
    /// Specifying the wrong length may lead to out-of-bounds reads and writes.
    pub unsafe fn from_raw_parts<E: CubePrimitive>(
        handle: &'a cubecl_runtime::server::Handle,
        length: usize,
        vectorization_factor: u8,
    ) -> Self {
        ArrayArg::Handle {
            handle: ArrayHandleRef::from_raw_parts(handle, length, E::as_elem().size()),
            vectorization_factor,
        }
    }

    /// Create a new array argument with a manual element size in bytes.
    ///
    /// # Safety
    ///
    /// Specifying the wrong length may lead to out-of-bounds reads and writes.
    pub unsafe fn from_raw_parts_and_size(
        handle: &'a cubecl_runtime::server::Handle,
        length: usize,
        vectorization_factor: u8,
        elem_size: usize,
    ) -> Self {
        ArrayArg::Handle {
            handle: ArrayHandleRef::from_raw_parts(handle, length, elem_size),
            vectorization_factor,
        }
    }

    /// Create an array from the corresponding Resource type of the Runtime.
    ///
    /// # Safety
    ///
    /// Highly unsafe as the caller has to ensure the resource is valid and is not aliased.
    pub unsafe fn from_raw_resource(
        resource: RawResource<<R::Server as ComputeServer>::Storage>,
    ) -> Self {
        ArrayArg::Resource(resource)
    }
}

impl<'a, R: Runtime> ArrayHandleRef<'a, R> {
    /// Create a new array handle reference.
    ///
    /// # Safety
    ///
    /// Specifying the wrong length may lead to out-of-bounds reads and writes.
    pub unsafe fn from_raw_parts(
        handle: &'a cubecl_runtime::server::Handle,
        length: usize,
        elem_size: usize,
    ) -> Self {
        Self {
            handle,
            length: [length],
            elem_size,
            runtime: PhantomData,
        }
    }

    /// Return the handle as a tensor instead of an array.
    pub fn as_tensor(&self) -> TensorHandleRef<'_, R> {
        let shape = &self.length;

        TensorHandleRef {
            handle: self.handle,
            strides: &[1],
            shape,
            elem_size: self.elem_size,
            runtime: PhantomData,
        }
    }
}

impl<C: CubePrimitive> LaunchArg for Array<C> {
    type RuntimeArg<'a, R: Runtime> = ArrayArg<'a, R>;

    fn compilation_arg<R: Runtime>(runtime_arg: &Self::RuntimeArg<'_, R>) -> Self::CompilationArg {
        match runtime_arg {
            ArrayArg::Handle {
                vectorization_factor,
                ..
            } => ArrayCompilationArg {
                inplace: None,
                vectorisation: Vectorization::Some(NonZero::new(*vectorization_factor).unwrap()),
            },
            ArrayArg::Alias { input_pos } => ArrayCompilationArg {
                inplace: Some(*input_pos as u16),
                vectorisation: Vectorization::None,
            },
            ArrayArg::Resource(_) => unimplemented!(),
        }
    }
}
