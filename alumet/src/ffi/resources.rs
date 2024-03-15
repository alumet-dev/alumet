use crate::resources::ResourceId;

// pub(crate) const RESOURCE_ID_SIZE: usize = std::mem::size_of::<ResourceId>();

#[repr(C)]
pub struct FfiResourceId {
    // Directly store the representation of ResourceId in memory, as a byte array of known size.
    // ResourceId is repr(C), therefore its size is guaranteed to stay the same
    // (unless its definition changes, of course, but the version of ALUMET will be increased if that happens,
    // and the plugins that have been compiled for an old version of ALUMET will be rejected.)
    bytes: [u8; 56], // should be RESOURCE_ID_SIZE but https://github.com/mozilla/cbindgen/issues/892
}

impl From<ResourceId> for FfiResourceId {
    fn from(value: ResourceId) -> Self {
        let bytes = unsafe { std::mem::transmute(value) };
        FfiResourceId { bytes }
    }
}

impl From<FfiResourceId> for ResourceId {
    fn from(value: FfiResourceId) -> Self {
        let bytes = value.bytes;
        unsafe { std::mem::transmute(bytes) }
    }
}

// ====== Constructors ======

#[no_mangle]
pub extern "C" fn resource_new_local_machine() -> FfiResourceId {
    ResourceId::LocalMachine.into()
}

#[no_mangle]
pub extern "C" fn resource_new_cpu_package(pkg_id: u32) -> FfiResourceId {
    ResourceId::CpuPackage { id: pkg_id }.into()
}

// ...
// TODO find a way to generate these automatically?
