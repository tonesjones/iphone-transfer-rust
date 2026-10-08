//! Read-only WPD access. COM objects stay on the thread that initialized COM.

use std::{io::Write, marker::PhantomData, rc::Rc};

use anyhow::{Context, Result, ensure};
use windows::{
    Win32::{
        Devices::PortableDevices::*,
        Foundation::{GENERIC_READ, PROPERTYKEY, S_FALSE, S_OK},
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize, STGM_READ,
        },
    },
    core::{Interface, PCWSTR, PWSTR, w},
};

const PHONE_HINT: &str = "keep the iPhone unlocked and tap Trust";

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub id: String,
    pub friendly_name: String,
    pub manufacturer: String,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub object_id: String,
    pub name: String,
    pub size: u64,
    pub is_folder: bool,
    pub path: String,
}

/// A successful MTA initialization, balanced on this same thread by Drop.
/// The Rc marker prevents moving the guard (and devices borrowing it) between threads.
pub struct Com {
    _thread: PhantomData<Rc<()>>,
}

impl Com {
    pub fn init() -> Result<Self> {
        // SAFETY: no reserved pointer is supplied; this thread owns the resulting guard.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        hr.ok().context("CoInitializeEx(COINIT_MULTITHREADED); an existing STA apartment cannot be changed to MTA")?;
        ensure!(
            hr == S_OK || hr == S_FALSE,
            "CoInitializeEx returned unexpected success {hr:?}"
        );
        Ok(Self {
            _thread: PhantomData,
        })
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        // SAFETY: a guard exists only after S_OK/S_FALSE, and cannot leave its initializing thread.
        unsafe { CoUninitialize() };
    }
}

// Own every returned allocation immediately, including partially filled arrays on failure.
struct TaskStrings(Vec<PWSTR>);

impl TaskStrings {
    fn new(count: usize) -> Self {
        Self(vec![PWSTR::null(); count])
    }

    fn string(&self, index: usize) -> Result<String> {
        let ptr = self.0[index];
        ensure!(!ptr.is_null(), "WPD returned a null string");
        // SAFETY: WPD returns NUL-terminated UTF-16 strings; this owner keeps the allocation alive.
        unsafe { ptr.to_string() }.context("decode WPD UTF-16 string")
    }
}

impl Drop for TaskStrings {
    fn drop(&mut self) {
        for ptr in &self.0 {
            // SAFETY: each non-null pointer is a unique WPD CoTaskMem allocation, freed once here.
            unsafe { CoTaskMemFree(Some(ptr.0.cast())) };
        }
    }
}

fn wide(value: &str) -> Result<Vec<u16>> {
    ensure!(!value.contains('\0'), "WPD identifier contains a NUL");
    Ok(value.encode_utf16().chain(Some(0)).collect())
}

// Manager strings use caller-owned buffers, not CoTaskMem allocations.
fn manager_text(
    call_name: &str,
    call: impl Fn(PWSTR, &mut u32) -> windows::core::Result<()>,
) -> Result<String> {
    let mut count = 0;
    call(PWSTR::null(), &mut count).with_context(|| format!("{call_name}(size); {PHONE_HINT}"))?;
    if count == 0 {
        return Ok(String::new());
    }
    let mut buffer = vec![0_u16; count as usize];
    call(PWSTR(buffer.as_mut_ptr()), &mut count)
        .with_context(|| format!("{call_name}(value); {PHONE_HINT}"))?;
    ensure!(
        count as usize <= buffer.len(),
        "{call_name} returned an invalid length"
    );
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16(&buffer[..end]).with_context(|| format!("decode {call_name}"))
}

fn devices_with_descriptions(_com: &Com) -> Result<Vec<(DeviceInfo, String)>> {
    // SAFETY: COM is initialized on this thread; no aggregation, valid class and interface IDs.
    let manager: IPortableDeviceManager =
        unsafe { CoCreateInstance(&PortableDeviceManager, None, CLSCTX_INPROC_SERVER) }
            .context("CoCreateInstance(PortableDeviceManager)")?;
    let mut count = 0;
    // SAFETY: a null array requests the count; count is a valid writable output.
    unsafe { manager.GetDevices(std::ptr::null_mut(), &mut count) }
        .with_context(|| format!("IPortableDeviceManager::GetDevices(count); {PHONE_HINT}"))?;
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut ids = TaskStrings::new(count as usize);
    // SAFETY: ids has count writable slots, and owns every pointer returned even on error.
    unsafe { manager.GetDevices(ids.0.as_mut_ptr(), &mut count) }
        .with_context(|| format!("IPortableDeviceManager::GetDevices(ids); {PHONE_HINT}"))?;
    ensure!(
        count as usize <= ids.0.len(),
        "GetDevices returned an invalid count"
    );
    let mut result = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let id = ids
            .string(i)
            .context("IPortableDeviceManager::GetDevices(id)")?;
        let id_wide = wide(&id)?;
        let id_ptr = PCWSTR(id_wide.as_ptr());
        let friendly_name = manager_text(
            "IPortableDeviceManager::GetDeviceFriendlyName",
            |buffer, count| {
                // SAFETY: id_ptr is live and terminated; manager_text supplies a null or sized buffer.
                unsafe { manager.GetDeviceFriendlyName(id_ptr, buffer, count) }
            },
        )?;
        let manufacturer = manager_text(
            "IPortableDeviceManager::GetDeviceManufacturer",
            |buffer, count| {
                // SAFETY: id_ptr is live and terminated; manager_text supplies a null or sized buffer.
                unsafe { manager.GetDeviceManufacturer(id_ptr, buffer, count) }
            },
        )?;
        let description = manager_text(
            "IPortableDeviceManager::GetDeviceDescription",
            |buffer, count| {
                // SAFETY: id_ptr is live and terminated; manager_text supplies a null or sized buffer.
                unsafe { manager.GetDeviceDescription(id_ptr, buffer, count) }
            },
        )?;
        result.push((
            DeviceInfo {
                id,
                friendly_name,
                manufacturer,
            },
            description,
        ));
    }
    Ok(result)
}

pub fn list_devices(com: &Com) -> Result<Vec<DeviceInfo>> {
    Ok(devices_with_descriptions(com)?
        .into_iter()
        .map(|(info, _)| info)
        .collect())
}

/// Match the friendly name or description, including user-renamed iPhones.
pub fn find_iphone(com: &Com) -> Result<Option<DeviceInfo>> {
    Ok(devices_with_descriptions(com)?
        .into_iter()
        .find_map(|(info, description)| {
            (info
                .friendly_name
                .to_ascii_lowercase()
                .contains("apple iphone")
                || description.to_ascii_lowercase().contains("apple iphone"))
            .then_some(info)
        }))
}

/// The borrow keeps COM initialized until all device interfaces have been released.
pub struct Device<'com> {
    device: IPortableDevice,
    _com: &'com Com,
}

impl<'com> Device<'com> {
    pub fn open(com: &'com Com, id: &str) -> Result<Self> {
        // SAFETY: COM is initialized on this thread; valid CLSIDs, with no aggregation.
        let device: IPortableDevice =
            unsafe { CoCreateInstance(&PortableDeviceFTM, None, CLSCTX_INPROC_SERVER) }
                .context("CoCreateInstance(PortableDeviceFTM)")?;
        // SAFETY: same initialized thread, valid values CLSID, no aggregation.
        let client: IPortableDeviceValues =
            unsafe { CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER) }
                .context("CoCreateInstance(PortableDeviceValues)")?;
        // SAFETY: client is valid, keys are static, and the literal is NUL-terminated.
        unsafe { client.SetStringValue(&WPD_CLIENT_NAME, w!("photoxfer-wpd")) }
            .context("IPortableDeviceValues::SetStringValue(WPD_CLIENT_NAME)")?;
        for (key, value, name) in [
            (WPD_CLIENT_MAJOR_VERSION, 0, "WPD_CLIENT_MAJOR_VERSION"),
            (WPD_CLIENT_MINOR_VERSION, 1, "WPD_CLIENT_MINOR_VERSION"),
            (WPD_CLIENT_REVISION, 0, "WPD_CLIENT_REVISION"),
            (
                WPD_CLIENT_DESIRED_ACCESS,
                GENERIC_READ.0,
                "WPD_CLIENT_DESIRED_ACCESS",
            ),
        ] {
            // SAFETY: client is valid and key points to a live PROPERTYKEY.
            unsafe { client.SetUnsignedIntegerValue(&key, value) }.with_context(|| {
                format!("IPortableDeviceValues::SetUnsignedIntegerValue({name})")
            })?;
        }
        let id_wide = wide(id)?;
        // SAFETY: the ID buffer and client info remain alive for this synchronous call.
        unsafe { device.Open(PCWSTR(id_wide.as_ptr()), &client) }
            .with_context(|| format!("IPortableDevice::Open; {PHONE_HINT}"))?;
        Ok(Self { device, _com: com })
    }

    /// Excludes DCIM itself; includes its descendant files and folders.
    /// Searches containers below DEVICE, so storage object IDs/names need not be hard-coded.
    pub fn list_dcim(&self) -> Result<Vec<Entry>> {
        // SAFETY: the open device and its COM apartment remain alive through self.
        let content = unsafe { self.device.Content() }
            .with_context(|| format!("IPortableDevice::Content; {PHONE_HINT}"))?;
        // SAFETY: content is a live COM interface on its initializing thread.
        let properties = unsafe { content.Properties() }
            .with_context(|| format!("IPortableDeviceContent::Properties; {PHONE_HINT}"))?;
        // SAFETY: COM is initialized; valid key collection CLSID and no aggregation.
        let keys: IPortableDeviceKeyCollection =
            unsafe { CoCreateInstance(&PortableDeviceKeyCollection, None, CLSCTX_INPROC_SERVER) }
                .context("CoCreateInstance(PortableDeviceKeyCollection)")?;
        for (key, name) in [
            (
                WPD_OBJECT_ORIGINAL_FILE_NAME,
                "WPD_OBJECT_ORIGINAL_FILE_NAME",
            ),
            (WPD_OBJECT_NAME, "WPD_OBJECT_NAME"),
            (WPD_OBJECT_SIZE, "WPD_OBJECT_SIZE"),
            (WPD_OBJECT_CONTENT_TYPE, "WPD_OBJECT_CONTENT_TYPE"),
        ] {
            // SAFETY: keys is valid and key lives through the synchronous Add call.
            unsafe { keys.Add(&key) }
                .with_context(|| format!("IPortableDeviceKeyCollection::Add({name})"))?;
        }
        // SAFETY: WPD_DEVICE_OBJECT_ID is a static, NUL-terminated binding constant.
        let root = unsafe { WPD_DEVICE_OBJECT_ID.to_string() }.context("WPD_DEVICE_OBJECT_ID")?;
        let mut pending = vec![(root, None::<String>)];
        let mut found_dcim = false;
        let mut entries = Vec::new();
        while let Some((parent, parent_path)) = pending.pop() {
            for object_id in child_ids(&content, &parent)? {
                let id_wide = wide(&object_id)?;
                // SAFETY: ID is terminated and live; properties and keys are valid interfaces.
                let values = unsafe { properties.GetValues(PCWSTR(id_wide.as_ptr()), &keys) }
                    .with_context(|| {
                        format!("IPortableDeviceProperties::GetValues({object_id}); {PHONE_HINT}")
                    })?;
                let name = property_string(&values, &WPD_OBJECT_ORIGINAL_FILE_NAME)
                    .ok().filter(|name| !name.is_empty())
                    .map(Ok)
                    .unwrap_or_else(|| property_string(&values, &WPD_OBJECT_NAME))
                    .with_context(|| format!("IPortableDeviceValues::GetStringValue(WPD_OBJECT_NAME, {object_id}); {PHONE_HINT}"))?;
                // SAFETY: values is live and the PROPERTYKEY is static.
                let content_type = unsafe { values.GetGuidValue(&WPD_OBJECT_CONTENT_TYPE) }
                    .with_context(|| format!("IPortableDeviceValues::GetGuidValue(WPD_OBJECT_CONTENT_TYPE, {object_id}); {PHONE_HINT}"))?;
                let is_folder = content_type == WPD_CONTENT_TYPE_FOLDER
                    || content_type == WPD_CONTENT_TYPE_FUNCTIONAL_OBJECT;
                // SAFETY: values is live and the PROPERTYKEY is static. Unsupported sizes default to 0.
                let size =
                    unsafe { values.GetUnsignedLargeIntegerValue(&WPD_OBJECT_SIZE) }.unwrap_or(0);
                if let Some(parent_path) = &parent_path {
                    let path = format!("{parent_path}/{name}");
                    if is_folder {
                        pending.push((object_id.clone(), Some(path.clone())));
                    }
                    entries.push(Entry {
                        object_id,
                        name,
                        size,
                        is_folder,
                        path,
                    });
                } else if is_folder {
                    let path = if name.eq_ignore_ascii_case("DCIM") {
                        found_dcim = true;
                        Some("DCIM".to_owned())
                    } else {
                        None
                    };
                    pending.push((object_id, path));
                }
            }
        }
        ensure!(found_dcim, "No DCIM folder found; {PHONE_HINT}");
        Ok(entries)
    }

    /// Streams one resource with bounded memory, continuing after short reads/S_FALSE.
    pub fn read_to<W: Write>(&self, object_id: &str, out: &mut W) -> Result<u64> {
        // SAFETY: self keeps the device and COM apartment alive.
        let content = unsafe { self.device.Content() }
            .with_context(|| format!("IPortableDevice::Content; {PHONE_HINT}"))?;
        // SAFETY: content is a live interface on this thread.
        let resources = unsafe { content.Transfer() }
            .with_context(|| format!("IPortableDeviceContent::Transfer; {PHONE_HINT}"))?;
        let id_wide = wide(object_id)?;
        let mut optimal = 0;
        let mut stream = None;
        // SAFETY: the ID/key are live; both outputs are valid, initialized writable locations.
        unsafe {
            resources.GetStream(
                PCWSTR(id_wide.as_ptr()),
                &WPD_RESOURCE_DEFAULT,
                STGM_READ.0,
                &mut optimal,
                &mut stream,
            )
        }
        .with_context(|| {
            format!("IPortableDeviceResources::GetStream({object_id}); {PHONE_HINT}")
        })?;
        let stream = stream.context("IPortableDeviceResources::GetStream returned no IStream")?;
        let buffer_size = if optimal == 0 { 256 * 1024 } else { optimal };
        let mut buffer = vec![0_u8; buffer_size as usize];
        let mut total = 0_u64;
        loop {
            let mut read = 0;
            // SAFETY: buffer has buffer_size writable bytes; read is a valid writable output.
            let hr =
                unsafe { stream.Read(buffer.as_mut_ptr().cast(), buffer_size, Some(&mut read)) };
            hr.ok()
                .with_context(|| format!("IStream::Read({object_id}); {PHONE_HINT}"))?;
            ensure!(
                read <= buffer_size,
                "IStream::Read returned an invalid byte count"
            );
            if read == 0 {
                break;
            }
            out.write_all(&buffer[..read as usize])
                .context("write WPD resource to output")?;
            total = total
                .checked_add(u64::from(read))
                .context("WPD resource byte count overflow")?;
        }
        Ok(total)
    }
}

fn child_ids(content: &IPortableDeviceContent, parent: &str) -> Result<Vec<String>> {
    let parent_wide = wide(parent)?;
    // SAFETY: parent ID is live and terminated; no filter; content is a valid interface.
    let enumerator = unsafe { content.EnumObjects(0, PCWSTR(parent_wide.as_ptr()), None) }
        .with_context(|| format!("IPortableDeviceContent::EnumObjects({parent}); {PHONE_HINT}"))?;
    let mut result = Vec::new();
    loop {
        let mut ids = TaskStrings::new(32);
        let mut fetched = 0;
        // SAFETY: ids has 32 writable slots; fetched is writable; RAII owns partial outputs too.
        let hr = unsafe { enumerator.Next(&mut ids.0, &mut fetched) };
        hr.ok().with_context(|| {
            format!("IEnumPortableDeviceObjectIDs::Next({parent}); {PHONE_HINT}")
        })?;
        ensure!(
            fetched as usize <= ids.0.len(),
            "Next returned an invalid count"
        );
        for i in 0..fetched as usize {
            result.push(
                ids.string(i)
                    .context("IEnumPortableDeviceObjectIDs::Next(id)")?,
            );
        }
        if hr == S_FALSE || fetched == 0 {
            break;
        }
    }
    Ok(result)
}

fn property_string(values: &IPortableDeviceValues, key: &PROPERTYKEY) -> Result<String> {
    let mut value = TaskStrings::new(1);
    // SAFETY: live interface/key and initialized writable PWSTR. Call the binding's vtable
    // directly so even a pointer returned with a failed HRESULT is owned and freed.
    let hr = unsafe { (values.vtable().GetStringValue)(values.as_raw(), key, &mut value.0[0]) };
    hr.ok().context("IPortableDeviceValues::GetStringValue")?;
    value.string(0)
}
