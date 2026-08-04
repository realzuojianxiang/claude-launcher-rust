// Grok OAuth token 加密存储。
//
// 设计目标：access_token / refresh_token **绝不以明文落盘**。config.json 只存授权
// 账号的 email 标识（GrokConfig::oauth_account），真正的 token 存储在本模块管理的
// `grok-oauth.json`，文件体只含 `{ account, blob }`：
//   - `blob` 是 DPAPI 对「JSON 序列化的 TokenStore」加密后的密文（base64）；
//   - 解密需当前 Windows 用户态（DPAPI CryptUnprotectData），跨用户/跨机器不可解，
//     既防偷文件直接用，也天然绑定本机账号。
//
// 落盘走 history.rs/config.rs 同款原子写（临时文件→flush→sync_all→rename），
// 损坏文件改名留证 + 回退 None，与 config/history 的「非静默回退」对齐。
// 删除用 tmp→sync→rename 失败时已残骸可见；本目录写入失败回执 Err。
//
// 平台：DPAPI 仅 Windows 有。非 Windows（CI 上的 `cargo check --tests`）走 fallback
// 分支：enc/dec 直接返回 Err，编译过但不提供功能（本项目实际只在 Windows 打包运行）。
// 这样 CI 门禁（check + clippy + fmt）绿，又不给非目标平台制造意外行为。
//
// 死代码允许可：本模块 Phase 3 起逐步被 3b(oauth.rs)/3d(mod.rs start)/3g(lib.rs
// grok_oauth_* 命令) 接线；接线完成前 pub 项暂未被 crate 外调用，统一放行 dead_code，
// 待 3d/3g 接入后随移除此 allow，与 proxy.rs/stream.rs 在各自接线前的做法一致。

#![allow(dead_code)]

use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// 上链回来的 token 包。明文本体——只在内存/DPAPI 密文中存在，不直接落盘。
///
/// `expires_at` 为 epoch 秒（0/过去表示未明确或已过期，由上层判断是否需 refresh）。
/// `account` 通常来自 id_token 的 email，作 UI 展示 + 与 GrokConfig::oauth_account 对齐的标识。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenStore {
    pub access_token: String,
    pub refresh_token: String,
    pub account: String,
    pub expires_at: i64,
}

/// 落盘外壳：`account` 仅作 UI 友好（可空），`blob` 是 DPAPI 密文的 base64。
/// 之所以把 account 也放外壳而非塞进加密体：损坏/未授权时前端要靠它判断「此前授权过哪个号」，
/// 而密文未解不建模。即使 account 被人看到也无害（email 不是凭证）。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredFile {
    account: String,
    blob: String,
}

// ---------------------------------------------------------------------------
// 路径 + 原子写（与 history.rs/config.rs 对齐）
// ---------------------------------------------------------------------------

/// token 文件路径：exe 同级 claude-launcher/grok-oauth.json。
pub fn path() -> PathBuf {
    let dir = Config::config_dir();
    let _ = fs::create_dir_all(&dir);
    dir.join("grok-oauth.json")
}

// ---------------------------------------------------------------------------
// DPAPI 加解密（仅 Windows；非 Windows 走编译能通过的 Err fallback）
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod dpapi {
    use std::ffi::c_void;

    // CryptProtectData/CryptUnprotectData 的 DATA_BLOB：cbData=字节长度，pbData=首字节指针。
    #[repr(C)]
    struct DataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }

    type Bool = i32;

    // dll 名: dpapi.h 实际由 crypt32.dll 导出；MSVC 链接器对 crypt32 的依赖在 Tauri
    // Windows 构建里通常已由系统库解析，显式 #[link] 兜底。
    #[link(name = "crypt32")]
    extern "system" {
        fn CryptProtectData(
            data_in: *const DataBlob,
            sz_data_descr: *const u16,
            optional_entropy: *const DataBlob,
            reserved: *const c_void,
            prompt_struct: *const c_void,
            flags: u32,
            data_out: *mut DataBlob,
        ) -> Bool;

        fn CryptUnprotectData(
            data_in: *const DataBlob,
            p_data_descr: *mut *mut u16,
            optional_entropy: *const DataBlob,
            reserved: *const c_void,
            prompt_struct: *const c_void,
            flags: u32,
            data_out: *mut DataBlob,
        ) -> Bool;
    }

    // 把 owned Vec<u8> 包成 DataBlob（pbData 指向 owned 缓冲首字节）。
    // 注意接口约定：CryptProtectData 不会释放调用方缓冲，调用期间保持 alive 即可。
    fn blob_from_vec(v: &mut Vec<u8>) -> DataBlob {
        // cbData 取 u32；超出 u32 的密文不该出现（token 远小于 4GiB）。截断语义不适用，
        // 直接 assert 排除异常输入而非默默截断。
        assert!(v.len() <= u32::MAX as usize, "DPAPI 输入过长");
        DataBlob {
            cb_data: v.len() as u32,
            pb_data: if v.is_empty() {
                std::ptr::null_mut()
            } else {
                v.as_mut_ptr()
            },
        }
    }

    // CryptProtectData 成功时把密文分配在 LocalFree 堆，由调用方 LocalFree 释放。
    // 用 LocalFree 释放 out.pbData。
    extern "system" {
        fn LocalFree(h: *mut c_void) -> *mut c_void;
    }

    /// 加密明文 → 密文。
    pub fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
        let mut in_buf = plain.to_vec();
        let in_blob = blob_from_vec(&mut in_buf);
        let mut out_blob = DataBlob {
            cb_data: 0,
            pb_data: std::ptr::null_mut(),
        };
        // 不传 description/entropy/prompt，flags=0。
        let ok = unsafe {
            CryptProtectData(
                &in_blob,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                &mut out_blob,
            )
        };
        if ok == 0 {
            return Err("DPAPI CryptProtectData 失败".to_string());
        }
        // 拷出密文再 LocalFree，确保不泄与缓冲越界。
        let cipher = unsafe {
            std::slice::from_raw_parts(out_blob.pb_data, out_blob.cb_data as usize).to_vec()
        };
        unsafe {
            LocalFree(out_blob.pb_data.cast());
        }
        Ok(cipher)
    }

    /// 解密密文 → 明文。
    pub fn unprotect(cipher: &[u8]) -> Result<Vec<u8>, String> {
        let mut in_buf = cipher.to_vec();
        let in_blob = blob_from_vec(&mut in_buf);
        let mut out_blob = DataBlob {
            cb_data: 0,
            pb_data: std::ptr::null_mut(),
        };
        let ok = unsafe {
            CryptUnprotectData(
                &in_blob,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                &mut out_blob,
            )
        };
        if ok == 0 {
            return Err("DPAPI CryptUnprotectData 失败".to_string());
        }
        let plain = unsafe {
            std::slice::from_raw_parts(out_blob.pb_data, out_blob.cb_data as usize).to_vec()
        };
        unsafe {
            LocalFree(out_blob.pb_data.cast());
        }
        Ok(plain)
    }
}

#[cfg(not(windows))]
mod dpapi {
    pub fn protect(_plain: &[u8]) -> Result<Vec<u8>, String> {
        Err("DPAPI 仅 Windows 可用；非 Windows 不支持 token 加密存储".to_string())
    }
    pub fn unprotect(_cipher: &[u8]) -> Result<Vec<u8>, String> {
        Err("DPAPI 仅 Windows 可用；非 Windows 不支持 token 加密存储".to_string())
    }
}

// base64：用成熟 base64 crate（standard alphabet + padding）包裹 DPAPI 密文落盘。
// 不手写实现——base64 细节多、易写错，复用成熟依赖更稳。
use base64::{engine::general_purpose::STANDARD, Engine as _};

fn b64_encode(data: &[u8]) -> String {
    STANDARD.encode(data)
}

fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    STANDARD
        .decode(s)
        .map_err(|e| format!("base64 解码失败: {e}"))
}

// ---------------------------------------------------------------------------
// 公共 API：save / load / clear
// ---------------------------------------------------------------------------

/// 把 token 包加密后原子写入。明文 token 仅在内存，落盘的是 DPAPI 密文 base64。
pub fn save(token: &TokenStore) -> Result<(), String> {
    let plain = serde_json::to_vec(token).map_err(|e| format!("序列化 token 失败: {e}"))?;
    let cipher = dpapi::protect(&plain)?;
    let blob = b64_encode(&cipher);
    let file = StoredFile {
        account: token.account.clone(),
        blob,
    };
    let data = serde_json::to_vec_pretty(&file).map_err(|e| format!("序列化外壳失败: {e}"))?;
    atomic_write_json(&data)
}

/// 读取并解密。文件缺失返回 None（未授权）；损坏改名留证并回退 None（对齐
/// config/history 的「非静默回退」——不让 token 文件悄悄损坏后无人知晓）。
pub fn load() -> Option<TokenStore> {
    let path = path();
    let data = match fs::read(&path) {
        Ok(d) => d,
        Err(_) => return None, // 缺文件：未授权，正常
    };
    let file: StoredFile = match serde_json::from_slice(&data) {
        Ok(f) => f,
        Err(e) => {
            rename_corrupt(&path, &e.to_string());
            return None;
        }
    };
    let cipher = match b64_decode(&file.blob) {
        Ok(c) => c,
        Err(e) => {
            rename_corrupt(&path, &format!("base64: {e}"));
            return None;
        }
    };
    match dpapi::unprotect(&cipher) {
        Ok(plain) => match serde_json::from_slice::<TokenStore>(&plain) {
            Ok(t) => Some(t),
            Err(e) => {
                rename_corrupt(&path, &format!("明文反序列化: {e}"));
                None
            }
        },
        Err(e) => {
            // 解密失败常因换用户/换机器（DPAPI 绑本机账号）。改名留证 + 回退 None，
            // 让上层提示重新授权，而不是把无效 token 当成有效。
            rename_corrupt(&path, &format!("DPAPI: {e}"));
            None
        }
    }
}

/// 清除本地 token（登出/重授权前调用）。文件不存在视为成功（幂等）。
pub fn clear() -> Result<(), String> {
    let path = path();
    if !path.exists() {
        return Ok(());
    }
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) => Err(format!("删除 grok-oauth.json 失败: {e}")),
    }
}

// ---------------------------------------------------------------------------
// 内部：原子写 + 损坏改名留证
// ---------------------------------------------------------------------------

fn atomic_write_json(data: &[u8]) -> Result<(), String> {
    let path = path();
    let dir = path
        .parent()
        .ok_or_else(|| "无法定位 token 目录".to_string())?;
    fs::create_dir_all(dir).map_err(|e| format!("创建 token 目录失败: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    {
        use std::io::Write;
        let mut f = fs::File::create(&tmp).map_err(|e| format!("创建临时 token 文件失败: {e}"))?;
        f.write_all(data)
            .map_err(|e| format!("写入 token 失败: {e}"))?;
        f.flush().map_err(|e| format!("刷新 token 失败: {e}"))?;
        let _ = f.sync_all();
    }
    if let Err(e) = fs::rename(&tmp, &path) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("原子替换 token 文件失败: {e}"));
    }
    Ok(())
}

fn rename_corrupt(path: &PathBuf, why: &str) {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let corrupt = path.with_extension(format!("corrupt-{stamp}.json"));
    let _ = fs::rename(path, &corrupt);
    tracing::error!(
        why = why,
        corrupt_path = ?corrupt,
        "grok-oauth.json 校验/解密失败，已重命名为证据文件并回退未授权"
    );
}

// ---------------------------------------------------------------------------
// 单测：Windows 上跑真 DPAPI 往返；非 Windows 只跑外壳逻辑（save 返回 Err 不落盘）。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn dpapi_roundtrip() {
        let plain = b"hello-grok-token-xyz";
        let cipher = dpapi::protect(plain).expect("CryptProtectData 应成功");
        assert_ne!(&cipher[..], plain, "密文不应等于明文");
        let back = dpapi::unprotect(&cipher).expect("CryptUnprotectData 应成功");
        assert_eq!(back, plain, "解密应还原明文");
    }

    #[cfg(windows)]
    #[test]
    fn save_load_roundtrip_on_real_dpapi() {
        // 用一个唯一 account，避免与可能存在的真实 token 文件冲突。
        // 注意：此测试会写真实 config_dir()/grok-oauth.json，跑完即 clear。
        let token = TokenStore {
            access_token: "xai-access-AAAAAAAA".to_string(),
            refresh_token: "xai-refresh-BBBBBBBB".to_string(),
            account: "roundtrip-test@example.com".to_string(),
            expires_at: 0,
        };
        save(&token).expect("save 应成功");
        let loaded = load().expect("load 应回来");
        assert_eq!(loaded.access_token, token.access_token);
        assert_eq!(loaded.refresh_token, token.refresh_token);
        assert_eq!(loaded.account, token.account);
        let _ = clear();
        assert!(load().is_none(), "clear 之后 load 应为 None");
    }

    #[cfg(windows)]
    #[test]
    fn unprotect_garbage_fails() {
        assert!(dpapi::unprotect(&[0u8; 32]).is_err(), "随机字节不应被解出");
    }

    #[test]
    fn base64_roundtrip() {
        for case in [
            b"".as_slice(),
            b"f",
            b"fo",
            b"foo",
            b"foob",
            b"fooba",
            b"foobar",
        ] {
            let enc = b64_encode(case);
            let dec = b64_decode(&enc).unwrap();
            assert_eq!(dec, case, "base64 往返 case len={}", case.len());
        }
        // 已知向量（RFC 4648，standard alphabet + padding）
        assert_eq!(b64_encode(b""), "");
        assert_eq!(b64_encode(b"f"), "Zg==");
        assert_eq!(b64_encode(b"fo"), "Zm8=");
        assert_eq!(b64_encode(b"foo"), "Zm9v");
        assert_eq!(b64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(b64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(b64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_rejects_illegal_input() {
        assert!(b64_decode("Zm9v!").is_err(), "含非法字符应报错");
        assert!(
            b64_decode("Zm9").is_err(),
            "长度非 4 倍数（缺 padding）应报错"
        );
    }
}
