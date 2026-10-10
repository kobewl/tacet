//! 会议检测 —— 「现在有没有应用在用麦克风」。
//!
//! 只问 CoreAudio 一个布尔量：**不录音、不打开任何音频流、不需要麦克风权限**。
//! 这和 macOS 状态栏上那个橙色麦克风指示点是同一类信息。
//!
//! ## 两条路径，优先精确的那条
//!
//! ### 路径 A：进程级（精确）
//!
//! 遍历 CoreAudio 的进程对象，看有没有哪个进程的
//! `kAudioProcessPropertyIsRunningInput` 为 1 —— 含义是「该进程正在运行 IO，
//! 且至少有一条**输入流**是活跃的」（SDK 头文件 `AudioHardware.h` 的原文说明）。
//! 单纯**播放**音乐 / 视频不会让它变 1。
//!
//! ### 路径 B：设备级（回退）
//!
//! 对默认输入设备读 `kAudioDevicePropertyDeviceIsRunningSomewhere`。
//! 它是**设备**级的、不区分输入输出：耳机 / AirPods 这类输入输出同属
//! 一个设备的硬件，播放音乐也会让它变成「运行中」，会产生误判。
//!
//! 所以 B 只在 A 不可用（系统不提供进程对象属性）时才用，
//! 并且误判的代价由上层的「单次推迟最多 90 分钟」保险丝封顶，
//! 见 `tacet_context::meeting`。
//!
//! 进程对象这组属性是较新的系统才有的；本模块**在运行时探测**
//! 它在当前系统上可不可用，而不是写死某个系统版本号。
//!
//! ## 为什么手写 FFI 而不是引入 `coreaudio-sys`
//!
//! 只用到三个函数、四个常量。`coreaudio-sys` 要靠 bindgen 生成整个框架的绑定，
//! 会显著拖慢冷构建并引入 libclang 依赖，对这点用量不划算。
//! 常量值逐个与本机 SDK 头文件（`AudioHardware.h` / `AudioHardwareBase.h`）核对过。

use std::ffi::c_void;
use std::mem::size_of;

use tacet_platform::{MeetingDetector, PlatformError};

type OsStatus = i32;
type AudioObjectId = u32;

/// `AudioObjectPropertyAddress`：「哪个对象的哪个属性」。
#[repr(C)]
#[derive(Clone, Copy)]
struct PropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectHasProperty(id: AudioObjectId, address: *const PropertyAddress) -> u8;

    fn AudioObjectGetPropertyDataSize(
        id: AudioObjectId,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        out_size: *mut u32,
    ) -> OsStatus;

    fn AudioObjectGetPropertyData(
        id: AudioObjectId,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        io_size: *mut u32,
        out_data: *mut c_void,
    ) -> OsStatus;
}

/// 把四字符码（如 `'prs#'`）转成 CoreAudio 用的 `u32`。
const fn fourcc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

/// `kAudioObjectSystemObject`。
const SYSTEM_OBJECT: AudioObjectId = 1;
/// `kAudioObjectUnknown`：哨兵值，没有任何对象的 ID 是 0。
const UNKNOWN_OBJECT: AudioObjectId = 0;
/// `kAudioObjectPropertyScopeGlobal` = `'glob'`。
const SCOPE_GLOBAL: u32 = fourcc(b"glob");
/// `kAudioObjectPropertyElementMain` = 0。
const ELEMENT_MAIN: u32 = 0;

/// `kAudioHardwarePropertyProcessObjectList` = `'prs#'`。
const PROCESS_OBJECT_LIST: u32 = fourcc(b"prs#");
/// `kAudioHardwarePropertyDefaultInputDevice` = `'dIn '`。
const DEFAULT_INPUT_DEVICE: u32 = fourcc(b"dIn ");
/// `kAudioProcessPropertyIsRunningInput` = `'piri'`。
const PROCESS_IS_RUNNING_INPUT: u32 = fourcc(b"piri");
/// `kAudioDevicePropertyDeviceIsRunningSomewhere` = `'gone'`。
const DEVICE_IS_RUNNING_SOMEWHERE: u32 = fourcc(b"gone");

const fn global(selector: u32) -> PropertyAddress {
    PropertyAddress {
        selector,
        scope: SCOPE_GLOBAL,
        element: ELEMENT_MAIN,
    }
}

fn has_property(id: AudioObjectId, address: &PropertyAddress) -> bool {
    // SAFETY: `address` 指向栈上一个有效的 `PropertyAddress`，调用期间不会失效；
    // 该函数只读取它，不保留指针。
    unsafe { AudioObjectHasProperty(id, address) != 0 }
}

/// 读一个 `u32` 属性；失败返回 `None`。
fn read_u32(id: AudioObjectId, address: &PropertyAddress) -> Option<u32> {
    let mut value: u32 = 0;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: `value` 是 4 字节的栈变量，`size` 如实告知了缓冲区大小，
    // 系统最多写入 `size` 字节；其余指针参数按文档传空。
    let status = unsafe {
        AudioObjectGetPropertyData(
            id,
            address,
            0,
            std::ptr::null(),
            &mut size,
            &mut value as *mut u32 as *mut c_void,
        )
    };
    (status == 0 && size as usize == size_of::<u32>()).then_some(value)
}

/// 读系统对象上的 `AudioObjectID` 数组（进程列表）。
fn read_object_list(id: AudioObjectId, address: &PropertyAddress) -> Option<Vec<AudioObjectId>> {
    let mut size: u32 = 0;
    // SAFETY: 先问需要多大缓冲区，`size` 是有效的输出变量。
    let status =
        unsafe { AudioObjectGetPropertyDataSize(id, address, 0, std::ptr::null(), &mut size) };
    if status != 0 {
        return None;
    }

    let count = size as usize / size_of::<AudioObjectId>();
    let mut ids = vec![UNKNOWN_OBJECT; count];
    if count == 0 {
        return Some(ids);
    }

    let mut io_size = (count * size_of::<AudioObjectId>()) as u32;
    // SAFETY: `ids` 的容量恰好是 `io_size` 字节，系统最多写入这么多；
    // 返回后用实际写入的字节数截断，避免读到未初始化的尾部。
    let status = unsafe {
        AudioObjectGetPropertyData(
            id,
            address,
            0,
            std::ptr::null(),
            &mut io_size,
            ids.as_mut_ptr() as *mut c_void,
        )
    };
    if status != 0 {
        return None;
    }
    ids.truncate(io_size as usize / size_of::<AudioObjectId>());
    Some(ids)
}

/// 路径 A：有没有进程正在采集输入。
///
/// 返回 `None` 表示**这个系统不提供进程级属性**（应当回退到路径 B），
/// 而不是「没有进程在用麦克风」—— 后者是 `Some(false)`。
fn process_level_input_running() -> Option<bool> {
    let list_address = global(PROCESS_OBJECT_LIST);
    if !has_property(SYSTEM_OBJECT, &list_address) {
        return None;
    }

    let processes = read_object_list(SYSTEM_OBJECT, &list_address)?;
    let probe = global(PROCESS_IS_RUNNING_INPUT);

    let mut supported = false;
    for process in processes {
        if !has_property(process, &probe) {
            continue;
        }
        supported = true;
        if read_u32(process, &probe) == Some(1) {
            return Some(true);
        }
    }

    // 一个进程都没有这个属性：当作系统不支持，让调用方回退。
    // （正常的系统上 coreaudiod 之外总有若干客户端进程，不会出现「支持但列表为空」。）
    supported.then_some(false)
}

/// 路径 B：默认输入设备「是否在某个进程里运行」。
fn device_level_input_running() -> Result<bool, PlatformError> {
    let default_input =
        read_u32(SYSTEM_OBJECT, &global(DEFAULT_INPUT_DEVICE)).ok_or_else(|| {
            PlatformError::System("读取默认输入设备失败（CoreAudio 返回错误）".to_string())
        })?;

    // 没有任何输入设备（如没接麦克风的台式 Mac）：当然没有人在用
    if default_input == UNKNOWN_OBJECT {
        return Ok(false);
    }

    let running =
        read_u32(default_input, &global(DEVICE_IS_RUNNING_SOMEWHERE)).ok_or_else(|| {
            PlatformError::System("读取输入设备运行状态失败（CoreAudio 返回错误）".to_string())
        })?;
    Ok(running != 0)
}

/// 麦克风是否被占用，并说明是用哪条路径判断的（诊断用）。
///
/// 返回 `(是否占用, 路径说明)`。
pub fn probe_microphone() -> Result<(bool, &'static str), PlatformError> {
    if let Some(in_use) = process_level_input_running() {
        return Ok((in_use, "进程级（kAudioProcessPropertyIsRunningInput）"));
    }
    device_level_input_running().map(|in_use| (in_use, "设备级回退（DeviceIsRunningSomewhere）"))
}

/// 会议检测的 macOS 实现。
///
/// 一次调用是几次纳秒到微秒级的只读系统查询，无状态、可跨线程共享。
#[derive(Default)]
pub struct MacMeetingDetector;

impl MeetingDetector for MacMeetingDetector {
    fn is_microphone_in_use(&self) -> tacet_platform::Result<bool> {
        probe_microphone().map(|(in_use, _)| in_use)
    }
}

/// 能力探测：这台机器上至少有一条路径可读。
pub(crate) fn capability_supported() -> bool {
    probe_microphone().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 四字符码与sdk头文件一致() {
        // 逐个对照 AudioHardware.h / AudioHardwareBase.h 里的字面量。
        assert_eq!(fourcc(b"glob"), 0x676C_6F62);
        assert_eq!(fourcc(b"prs#"), 0x7072_7323);
        assert_eq!(fourcc(b"dIn "), 0x6449_6E20);
        assert_eq!(fourcc(b"piri"), 0x7069_7269);
        assert_eq!(fourcc(b"gone"), 0x676F_6E65);
    }

    #[test]
    fn 属性地址结构与c布局一致() {
        // AudioObjectPropertyAddress 是三个 UInt32，共 12 字节。
        assert_eq!(size_of::<PropertyAddress>(), 12);
    }

    #[test]
    fn 真实调用不会崩溃() {
        // CI 的 macOS runner 可能没有音频设备，结果是 true / false / Err 都合理，
        // 取决于运行环境。这条只守住「调用本身不崩溃」。
        let _ = MacMeetingDetector.is_microphone_in_use();
        let _ = capability_supported();
    }
}
