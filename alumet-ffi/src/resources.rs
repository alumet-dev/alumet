use alumet::resources::{Resource, ResourceConsumer};

// pub(crate) const RESOURCE_ID_SIZE: usize = std::mem::size_of::<ResourceId>();

#[repr(C)]
pub struct FfiResourceId {
    // Directly store the representation of ResourceId in memory, as a byte array of known size.
    // ResourceId is repr(C), therefore its size is guaranteed to stay the same
    // (unless its definition changes, of course, but the version of ALUMET will be increased if that happens,
    // and the plugins that have been compiled for an old version of ALUMET will be rejected.)
    bytes: [u8; 56], // should be RESOURCE_ID_SIZE but https://github.com/mozilla/cbindgen/issues/892
}

impl From<Resource> for FfiResourceId {
    fn from(value: Resource) -> Self {
        let bytes = unsafe { std::mem::transmute(value) };
        FfiResourceId { bytes }
    }
}

impl From<FfiResourceId> for Resource {
    fn from(value: FfiResourceId) -> Self {
        let bytes = value.bytes;
        unsafe { std::mem::transmute(bytes) }
    }
}

#[repr(C)]
pub struct FfiConsumerId {
    bytes: [u8; 56], // same problem as above
}

impl From<ResourceConsumer> for FfiConsumerId {
    fn from(value: ResourceConsumer) -> Self {
        let bytes = unsafe { std::mem::transmute(value) };
        FfiConsumerId { bytes }
    }
}

impl From<FfiConsumerId> for ResourceConsumer {
    fn from(value: FfiConsumerId) -> Self {
        let bytes = value.bytes;
        unsafe { std::mem::transmute(bytes) }
    }
}

// ====== Constructors ======

// TODO find a way to generate these automatically?

#[no_mangle]
pub extern "C" fn resource_new_local_machine() -> FfiResourceId {
    Resource::LocalMachine.into()
}

#[no_mangle]
pub extern "C" fn resource_new_cpu_package(pkg_id: u32) -> FfiResourceId {
    Resource::CpuPackage { id: pkg_id }.into()
}

#[no_mangle]
pub extern "C" fn consumer_new_local_machine() -> FfiConsumerId {
    ResourceConsumer::LocalMachine.into()
}

#[no_mangle]
pub extern "C" fn consumer_new_process(pid: u32) -> FfiConsumerId {
    ResourceConsumer::Process { pid }.into()
}

// ====== Tests ======

#[cfg(test)]
mod tests {
    use crate::resources::{Resource, ResourceConsumer};

    #[test]
    fn test_memory_layout() {
        assert_eq!(56, std::mem::size_of::<Resource>());
        assert_eq!(56, std::mem::size_of::<ResourceConsumer>());
    }
}
