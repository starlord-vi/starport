//! Native Windows conformance fixtures, not the production readback handler.
//! In particular the private-page fixture opens a page without publishing GPU
//! data; only the mapped-alias fixture demonstrates protected publication.
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::Diagnostics::Debug::*;
use windows_sys::Win32::System::Memory::*;

static SERIAL: Mutex<()> = Mutex::new(());
static BASE: AtomicUsize = AtomicUsize::new(0);
static ALIAS: AtomicUsize = AtomicUsize::new(0);
static TOUCHED: AtomicUsize = AtomicUsize::new(0);
static PROTECTED_DURING_PUBLISH: AtomicUsize = AtomicUsize::new(0);
const PAGE: usize = 4096;

unsafe extern "system" fn fault(info: *mut EXCEPTION_POINTERS) -> i32 {
    // SAFETY: Windows supplies the record; no allocation or application locks.
    let record = unsafe { &*(*info).ExceptionRecord };
    if !matches!(record.ExceptionCode as u32, 0xc0000005 | 0x80000001)
        || record.NumberParameters < 2
        || record.ExceptionInformation[0] > 1
    {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    let address = record.ExceptionInformation[1];
    let base = BASE.load(Ordering::Acquire);
    if base == 0 || address < base || address >= base + 2 * PAGE {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    let index = (address - base) / PAGE;
    let page = base + index * PAGE;
    let alias = ALIAS.load(Ordering::Acquire);
    if alias != 0 {
        let mut before: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        let queried = unsafe { VirtualQuery(page as _, &mut before, size_of_val(&before)) };
        if queried == 0 || before.Protect != PAGE_NOACCESS {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        // Mock already-completed GPU bytes. The host-only alias is writable
        // while the guest view remains inaccessible to every guest reader.
        unsafe {
            std::ptr::write_bytes((alias + index * PAGE) as *mut u8, 0xa0 + index as u8, PAGE)
        };
        let mut after: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { VirtualQuery(page as _, &mut after, size_of_val(&after)) } != 0
            && after.Protect == PAGE_NOACCESS
        {
            PROTECTED_DURING_PUBLISH.fetch_or(1 << index, Ordering::Release);
        }
    }
    let mut old = 0;
    if unsafe { VirtualProtect(page as _, PAGE, PAGE_READWRITE, &mut old) } == 0 {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    TOUCHED.fetch_or(1 << index, Ordering::Release);
    EXCEPTION_CONTINUE_EXECUTION
}

struct Handler(*mut std::ffi::c_void);
impl Handler {
    fn install(base: usize, alias: usize) -> Self {
        TOUCHED.store(0, Ordering::Release);
        PROTECTED_DURING_PUBLISH.store(0, Ordering::Release);
        ALIAS.store(alias, Ordering::Release);
        BASE.store(base, Ordering::Release);
        let handler = unsafe { AddVectoredExceptionHandler(1, Some(fault)) };
        assert!(!handler.is_null());
        Self(handler)
    }
}
impl Drop for Handler {
    fn drop(&mut self) {
        BASE.store(0, Ordering::Release);
        ALIAS.store(0, Ordering::Release);
        unsafe { RemoveVectoredExceptionHandler(self.0) };
    }
}

#[test]
fn guard_and_noaccess_faults_compose_with_native_private_write_watch() {
    let _serial = SERIAL.lock().unwrap();
    struct Allocation(*mut std::ffi::c_void);
    impl Drop for Allocation {
        fn drop(&mut self) {
            unsafe { VirtualFree(self.0, 0, MEM_RELEASE) };
        }
    }
    let allocation = Allocation(unsafe {
        VirtualAlloc(
            std::ptr::null(),
            64 << 10,
            MEM_RESERVE | MEM_COMMIT | 0x20_0000,
            PAGE_READWRITE,
        )
    });
    assert!(!allocation.0.is_null());
    unsafe { allocation.0.cast::<u8>().write_bytes(0x3c, 64 << 10) };
    let base = allocation.0 as usize;
    let _handler = Handler::install(base, 0);
    let mut found = [std::ptr::null_mut(); 16];
    let mut count = found.len();
    let mut granularity = 0;
    assert_eq!(
        unsafe {
            GetWriteWatch(
                1,
                allocation.0,
                64 << 10,
                found.as_mut_ptr(),
                &mut count,
                &mut granularity,
            )
        },
        0
    );
    let mut old = 0;
    assert_ne!(
        unsafe { VirtualProtect(allocation.0, PAGE, PAGE_READWRITE | PAGE_GUARD, &mut old) },
        0
    );
    assert_ne!(
        unsafe { VirtualProtect((base + PAGE) as _, PAGE, PAGE_NOACCESS, &mut old) },
        0
    );
    count = found.len();
    assert_eq!(
        unsafe {
            GetWriteWatch(
                0,
                allocation.0,
                64 << 10,
                found.as_mut_ptr(),
                &mut count,
                &mut granularity,
            )
        },
        0,
        "write-watch queries must work while pages are protected"
    );
    assert_eq!(count, 0);
    assert_eq!(unsafe { std::ptr::read_volatile(base as *const u8) }, 0x3c);
    assert_eq!(
        unsafe { std::ptr::read_volatile((base + PAGE) as *const u8) },
        0x3c
    );
    assert_eq!(TOUCHED.load(Ordering::Acquire), 3);
    unsafe { std::ptr::write_volatile((base + PAGE + 8) as *mut u8, 77) };
    count = found.len();
    assert_eq!(
        unsafe {
            GetWriteWatch(
                0,
                allocation.0,
                64 << 10,
                found.as_mut_ptr(),
                &mut count,
                &mut granularity,
            )
        },
        0
    );
    assert_eq!(count, 1);
    assert_eq!(found[0] as usize, base + PAGE);
    eprintln!(
        "READBACK_PROBE private MEM_WRITE_WATCH: PAGE_GUARD and PAGE_NOACCESS reads fault correctly; later write retained"
    );
}

#[test]
fn mapped_host_alias_publishes_only_the_faulted_page_while_guest_stays_noaccess() {
    let _serial = SERIAL.lock().unwrap();
    struct Views {
        guest: MEMORY_MAPPED_VIEW_ADDRESS,
        host: MEMORY_MAPPED_VIEW_ADDRESS,
        section: HANDLE,
    }
    impl Drop for Views {
        fn drop(&mut self) {
            unsafe {
                UnmapViewOfFile(self.guest);
                UnmapViewOfFile(self.host);
                CloseHandle(self.section)
            };
        }
    }
    let section = unsafe {
        CreateFileMappingW(
            INVALID_HANDLE_VALUE,
            std::ptr::null(),
            PAGE_READWRITE | SEC_RESERVE,
            0,
            64 << 10,
            std::ptr::null(),
        )
    };
    assert!(!section.is_null());
    let guest = unsafe { MapViewOfFile(section, FILE_MAP_WRITE, 0, 0, 64 << 10) };
    let host = unsafe { MapViewOfFile(section, FILE_MAP_WRITE, 0, 0, 64 << 10) };
    assert!(!guest.Value.is_null());
    assert!(!host.Value.is_null());
    let views = Views {
        guest,
        host,
        section,
    };
    assert_eq!(
        unsafe { VirtualAlloc(guest.Value, 64 << 10, MEM_COMMIT, PAGE_READWRITE) },
        guest.Value
    );
    unsafe { host.Value.cast::<u8>().write_bytes(0x3c, 64 << 10) };
    let base = guest.Value as usize;
    let _handler = Handler::install(base, host.Value as usize);
    let mut old = 0;
    assert_ne!(
        unsafe { VirtualProtect(guest.Value, 2 * PAGE, PAGE_NOACCESS, &mut old) },
        0
    );
    assert_eq!(unsafe { std::ptr::read_volatile(base as *const u8) }, 0xa0);
    assert_eq!(TOUCHED.load(Ordering::Acquire), 1);
    assert_eq!(PROTECTED_DURING_PUBLISH.load(Ordering::Acquire), 1);
    assert_eq!(
        unsafe { *host.Value.cast::<u8>().add(PAGE) },
        0x3c,
        "untouched page has not been merged"
    );
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    assert_ne!(
        unsafe { VirtualQuery((base + PAGE) as _, &mut info, size_of_val(&info)) },
        0
    );
    assert_eq!(info.Protect, PAGE_NOACCESS);
    assert_eq!(
        unsafe { std::ptr::read_volatile((base + PAGE) as *const u8) },
        0xa1
    );
    assert_eq!(PROTECTED_DURING_PUBLISH.load(Ordering::Acquire), 3);
    let mut found = [std::ptr::null_mut(); 16];
    let mut count = found.len();
    let mut granularity = 0;
    let status = unsafe {
        GetWriteWatch(
            0,
            guest.Value,
            64 << 10,
            found.as_mut_ptr(),
            &mut count,
            &mut granularity,
        )
    };
    assert_ne!(
        status, 0,
        "mapped aliases require a software dirty journal instead of MEM_WRITE_WATCH"
    );
    eprintln!(
        "READBACK_PROBE mapped alias: protected page publication works; native GetWriteWatch status={status}; software dirty tracking required"
    );
    drop(_handler);
    drop(views);
}
