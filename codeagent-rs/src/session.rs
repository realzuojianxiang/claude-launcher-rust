// session —— P7:会话持久化(保存/恢复 `Vec<Message>`)。
//
// 设计(见 journey §10):
//   · 落盘 = 整条历史 JSON(Vec<Message> 已 #[derive(Serialize, Deserialize)],直接序列化)。
//     Message 里不含 reasoning_content(§3.3 #3 考量本就不进历史),落盘干净无思考污染。
//   · 原子写(临时文件 → 写 → flush → sync_all → rename),防「写到一半挂留下半截 JSON」。
//   · 损坏文件「改名留证」不静默回退 —— 是 codeagent-rs 这里**首次**引入这个模式
//     (launcher rust/ 那边的历史模块已有同思路,本 crate 此前没有;P7 立标杆)。
//     坏文件改名带点前缀(.corrupt.<原名>)降低被 glob 误抓;绝不默默吞掉载入失败。
//
// 只动文件 IO,不碰网络/模型 —— 故可被单测硬证往返保真 / 损坏留证 / 首跑无文件等边界。
// 真跨进程「quit → 重启 → resume 接着聊」要真终端,留本机实测印记(P5.5 第 4 组同结构)。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::Message;

/// 落盘结构:版本号 + 全量历史。版本号给日后 schema 变更留迁移口(现仅 v1)。
/// 单独包一层(不直接序列化 Vec<Message>)是为了有个稳定的「外壳」字段位,
/// 以后加 created_at / provider 名 / model 名 等元数据时不必破坏旧文件的兼容。
#[derive(Serialize, Deserialize)]
struct SessionFile {
    /// 文件格式版本。现固定 "1"。读时若遇未知版本 → 报错载入失败(绝不默默猜)。
    version: String,
    /// 全量对话历史(system + user/assistant/tool 全在,顺序即对话顺序)。
    messages: Vec<Message>,
}

/// 把整条历史原子落盘到 `path`。
/// 临时文件名 = 同目录下 `.<原文件名>.tmp`(点开头降低被glob误抓),写完 fsync 再 rename,
/// 保证要么旧文件完整在、要么新文件完整在 —— 不会有「写到半空的 JSON」中间态。
pub fn save(path: &Path, messages: &[Message]) -> Result<()> {
    let sess = SessionFile {
        version: "1".to_string(),
        messages: messages.to_vec(),
    };
    let json = serde_json::to_string_pretty(&sess)
        .with_context(|| "会话序列化失败(消息列表含不可序列化字段?)")?;

    let dir = path
        .parent()
        .with_context(|| format!("会话文件路径无父目录: {}", path.display()))?;
    let file_name = path
        .file_name()
        .with_context(|| format!("会话文件路径无文件名: {}", path.display()))?;
    let mut tmp = PathBuf::from(format!(".{}.tmp", file_name.to_string_lossy()));
    tmp = dir.join(tmp);

    // 1) 写临时文件 → 2) flush + fsync(确保数据真落盘,不只是页缓存) → 3) rename 原子顶替。
    // 撑过「写到一半进程被杀 / 断电」:旧文件要么完整在、要么新文件完整在,没中间态。
    {
        let mut f = std::fs::File::create(&tmp)
            .with_context(|| format!("建临时会话文件失败: {}", tmp.display()))?;
        use std::io::Write;
        f.write_all(json.as_bytes())
            .with_context(|| format!("写临时会话文件失败: {}", tmp.display()))?;
        f.flush()
            .with_context(|| format!("flush 临时会话文件失败: {}", tmp.display()))?;
        // sync_all 在某些 FS(网络盘 / 某些移动介质)可能不支持,记一步但放行:
        // rename 成功后即便没 sync 到底,旧文件也还在,最差是「这次没存上」,不是「存坏了」。
        if let Err(e) = f.sync_all() {
            eprintln!("[note] 临时会话文件 sync_all 不支持,跳过 fsync(数据仍在页缓存,理论上有断电丢风险): {e}");
        }
    }
    // Windows: 若目标已存在,fs::rename 会失败(不像 POSIX 那样覆盖)。先尝试直接 rename,
    // 失败时退「先删目标再 rename」一条。两条都失败才报错 —— 走这条是因为 Windows 的
    // tmpfs rename 不覆盖,而我们是唯一写者(本进程独占会话文件),先删是安全的。
    if let Err(e) = std::fs::rename(&tmp, path) {
        // 目标存在 → 删了再 rename;目标不存在但 rename 仍失败(权限/占用)→ 纯报错。
        if path.exists() {
            std::fs::remove_file(path).with_context(|| {
                format!(
                    "rename 顶替旧会话失败,删旧会话也失败(rename 原因: {e}): {}",
                    path.display()
                )
            })?;
            std::fs::rename(&tmp, path)
                .with_context(|| format!("删旧后再 rename 临时会话失败: {}", tmp.display()))?;
        } else {
            return Err(e).with_context(|| format!("rename 临时会话 → {} 失败", path.display()));
        }
    }
    Ok(())
}

/// 从 `path` 载入历史。三类情形三类反应(不静默吞载入失败的纪律):
///   1. 文件不存在 → 返回 None(首跑 / 从未存过,正常)。
///   2. 文件存在但损坏 → 报错 + **改名留证**(.corrupt.<原名>),把坏文件留给人查,不静默吞。
///   3. 正常 → 返回 Some(messages)。
pub fn load(path: &Path) -> Result<Option<Vec<Message>>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("读取会话文件失败: {}", path.display())),
    };
    let sess: SessionFile = match serde_json::from_str(&text) {
        Ok(s) => s,
        Err(e) => {
            // 损坏留证:不改名悄悄丢,保留现场。点前缀降低被 glob 误抓。
            let corrupt_name = format!(
                ".corrupt.{}",
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "session".into())
            );
            let corrupt_path = path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&corrupt_name);
            // 改名失败不致命(文件可能被锁),记一笔后照常报"载入失败",让上层决定降级。
            if let Err(ce) = std::fs::rename(path, &corrupt_path) {
                eprintln!(
                    "[note] 会话文件损坏且改名留证失败(可能被占用): {ce} —— 坏文件留在原位 {}",
                    path.display()
                );
            }
            return Err(anyhow::anyhow!(
                "会话文件损坏: 解析失败 ({})。坏文件已改名留证至 {}。",
                e,
                corrupt_path.display()
            ));
        }
    };
    // 版本闸:遇未知版本**不猜**,报错让人知晓(schema 变更时要显式迁移)。
    if sess.version != "1" {
        return Err(anyhow::anyhow!(
            "会话文件版本为 \"{}\",本版仅支持 \"1\"。可能是由更新版本写入 —— 不静默猜,需显式迁移。",
            sess.version
        ));
    }
    Ok(Some(sess.messages))
}

#[cfg(test)]
mod tests {
    //! P7 会话持久化 —— 纯文件 IO 测试,不联网、不调模型、不需真终端。
    //! 锁住两条易回归的硬点:(1) 往返保真 —— 存盘再载入,Message 列表逐字符相等;
    //! (2) 损坏留证 —— 坏 JSON 不被静默吞,改名成 .corrupt.* 留现场。
    //! 还覆盖首跑无文件(返 None)、空会话、版本闸三项边界。
    //! 用临时目录隔离:每个测试自建专属目录,assert 后清理(不依赖测试运行顺序、不污染 repo)。
    use super::*;
    use crate::Message;

    /// 帮手:在本进程临时区建一个**专用并存在**的目录,返回它的路径。每个测试一个独立目录。
    /// 调用方传 sub(如 "roundtrip"),返回 temp/codeagent-test-<pid>/<sub>,并**保证它存在**
    /// (会 create_dir_all)—— save() 内部 File::create 不建父目录,故这里先把父建好。
    fn unique_tmp_dir(sub: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("codeagent-test-{}", std::process::id()));
        dir.push(sub);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn save_then_load_roundtrips_messages_exactly() {
        let dir = unique_tmp_dir("roundtrip");
        let path = dir.join("session.json");
        let msgs = vec![
            Message::system("你是一个简洁的 code agent。"),
            Message::user("你好"),
            // assistant 带 content + tool_calls 的复合格态,验证 tool_calls 往返不丢。
            Message {
                role: "assistant".into(),
                content: "已读取文件".into(),
                tool_calls: Some(vec![crate::tools::ToolCall {
                    id: "call_42".into(),
                    r#type: "function".into(),
                    function: crate::tools::ToolCallFunction {
                        name: "read_file".into(),
                        arguments: r#"{"path":"hello.txt"}"#.into(),
                    },
                }]),
                tool_call_id: None,
            },
            // role:tool 配对 tool_call_id,验证配对 id 往返不丢。
            Message {
                role: "tool".into(),
                content: "codeagent P3 生效".into(),
                tool_calls: None,
                tool_call_id: Some("call_42".into()),
            },
        ];
        save(&path, &msgs).unwrap();
        // 文件落盘生效 —— 存在且可读为文本(非空)。
        assert!(path.exists(), "save 后会话文件应存在");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"version\": \"1\""), "落盘应带 version 字段");
        assert!(
            text.contains("call_42"),
            "assistant 的 tool_call id 应在落盘里"
        );

        let loaded = load(&path).unwrap().expect("正常文件应载入为 Some");
        assert_eq!(loaded.len(), msgs.len(), "往返条数应一致");
        for (i, (orig, got)) in msgs.iter().zip(loaded.iter()).enumerate() {
            assert_eq!(orig.role, got.role, "第 {i} 条 role 往返不等");
            assert_eq!(orig.content, got.content, "第 {i} 条 content 往返不等");
            assert_eq!(
                orig.tool_call_id, got.tool_call_id,
                "第 {i} 条 tool_call_id 往返不等"
            );
            assert_eq!(
                orig.tool_calls.is_some(),
                got.tool_calls.is_some(),
                "第 {i} 条 tool_calls 存在性往返不等",
            );
            if let (Some(a), Some(b)) = (orig.tool_calls.as_ref(), got.tool_calls.as_ref()) {
                assert_eq!(a.len(), b.len(), "第 {i} 条 tool_calls 数量往返不等");
                for (j, (ac, bc)) in a.iter().zip(b.iter()).enumerate() {
                    assert_eq!(ac.id, bc.id, "第 {i} 条第 {j} 个 tool_call id 往返不等");
                    assert_eq!(
                        ac.function.name, bc.function.name,
                        "第 {i} 条第 {j} 个 tool_call name 往返不等"
                    );
                    assert_eq!(
                        ac.function.arguments, bc.function.arguments,
                        "第 {i} 条第 {j} 个 tool_call arguments 往返不等"
                    );
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_returns_none_when_file_missing() {
        // 首跑 / 从未存过:load 不应报错,应优雅返回 None(上层据此跳过 resume)。
        let dir = unique_tmp_dir("missing");
        let path = dir.join("does-not-exist.json");
        assert!(!path.exists());
        let loaded = load(&path).unwrap();
        assert!(loaded.is_none(), "不存在的会话文件应返回 None 而非报错");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_then_save_again_overwrites_atomically() {
        // 同一会话文件被多轮反复 save(REPL 每轮落盘)应能安全顶替旧内容,不留临时残留。
        let dir = unique_tmp_dir("overwrite");
        let path = dir.join("session.json");
        let msgs_a = vec![Message::user("第一句")];
        let msgs_b = vec![Message::user("第一句"), Message::user("第二句")];
        save(&path, &msgs_a).unwrap();
        save(&path, &msgs_b).unwrap();
        // 临时文件应已被 rename 走,目录里只剩正式会话文件。
        let entries: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert_eq!(
            entries.len(),
            1,
            "反复 save 后目录应只有 1 个正式文件,无临时残留"
        );
        let loaded = load(&path).unwrap().unwrap();
        assert_eq!(loaded.len(), 2, "二次 save 应顶替为 2 条而非保留旧 1 条");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_session_roundtrips() {
        // 空会话(从未发过消息就 quit)也应能存 —— 载入回来是空列表,不是 None(文件确实存在且合法)。
        let dir = unique_tmp_dir("empty");
        let path = dir.join("session.json");
        save(&path, &[]).unwrap();
        let loaded = load(&path).unwrap();
        assert!(
            loaded.is_some(),
            "空会话文件存在且合法应载入为 Some(空列表),不是 None"
        );
        assert!(loaded.unwrap().is_empty(), "空会话载入应是空列表");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_is_renamed_not_silently_swallowed() {
        // 损坏留证:坏 JSON 应报错(不静默),并把坏文件改名成 .corrupt.* 留现场 —— 原文件不再在原位。
        let dir = unique_tmp_dir("corrupt");
        let path = dir.join("session.json");
        std::fs::write(&path, "这不是合法 JSON <<<>>>").unwrap();
        let res = load(&path);
        assert!(res.is_err(), "损坏的会话文件应报错而非静默吞掉");
        let err_msg = format!("{}", res.unwrap_err());
        assert!(
            err_msg.contains("损坏") || err_msg.contains("解析失败"),
            "错误信息应点明是损坏:{err_msg}",
        );
        // 损坏留证:原文件不在原位 → 改名走过了(理论极端:文件被锁导致改名失败那条会把它留原位,
        // 但这个测试里没锁,改名应成功,故可断言原文件已不在)。
        assert!(
            !path.exists(),
            "损坏文件应被改名留证,原位应不再有 session.json(实际:可能改名为 .corrupt.session.json)",
        );
        // 且留证文件存在于同目录(改名产物)。
        let corrupt_path = dir.join(".corrupt.session.json");
        assert!(
            corrupt_path.exists(),
            "损坏留证文件应存在于 .corrupt.session.json"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_version_is_rejected_not_guessed() {
        // 版本闸:手写一个 version="99" 的合法 JSON,load 应报错而非默默猜。
        let dir = unique_tmp_dir("version");
        let path = dir.join("session.json");
        std::fs::write(&path, r#"{"version":"99","messages":[]}"#).unwrap();
        let res = load(&path);
        assert!(res.is_err(), "未知版本号应报错而非静默猜");
        let err_msg = format!("{}", res.unwrap_err());
        assert!(
            err_msg.contains("99"),
            "版本闸错误应点明收到的版本号: {err_msg}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
