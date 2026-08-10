//! (a) Phase A §2.4:SubagentTool 真 spawn 真 key 端到端 —— 起一个 `codeagent --script --yolo`
//! 子进程、喂一句「用一句话回答:好」一次性 prompt、收它 stdout(模型终答)、断言回灌有
//! `[subagent 答复]` 包裹。是现 `.e2e/` 手动 1-leg 回贴的 cargo-test 自跑类比:journey 记
//! 「1-leg 已本机跑通回贴」,但手跑要记得才能验 —— 现在有 key 时 `cargo test` 自动验。
//!
//! 这是 `(c′) 桥 block_on_current` 最吓人路径之一(SubagentTool::execute 现经桥跑 async
//! spawn + write_all + read_to_end + wait)的真 IO 路径。Phase A 基线 = 经桥绿;Phase B 删桥
//! 后 `execute` 变 async、不经桥也在多线程 runtime 上直接 `.await` —— 两边都绿才证 Subagent
//! async spawn 路径**运行期**等价(非只编译),套住 §14.3「门禁绿=伪绿」坑。
//!
//! bin 注入:不调 `SubagentTool::new()`(它走 `current_exe()`,cargo test 里那是**测试运行器
//! 二进制**不是 codeagent CLI,会错起自己);改调 [`SubagentTool::new_with_bin`] 显式塞
//! `env!("CARGO_BIN_EXE_codeagent")`(真 codeagent exe,cargo 给集成测试设的绝对路径)。
//!
//! 子进程 cwd 继承本进程 cwd(cargo test 期 = crate root),故读 crate root 的 `codeagent.toml`
//! (用户真生产配置,gitignored 未追踪;**不动它**——只读其 provider 段让子进程拿 model/base_url/
//! api_key_env;真 key 子进程继承父 env 的 `DEEPSEEK_API_KEY`,不再显式注入)。prompt「用一句话
//! 回答:好」不催 tool_call,子进程一轮模型答完直接退(EOF 收工),不写盘 —— 无副作用、无残留。
//!
//! Gate(CODEAGENT_E2E + DEEPSEEK_API_KEY)未就位 → skip(早 return,非 `#[ignore]`),见
//! `common::guard_e2e_or_skip`。
//!
//! `flavor = "multi_thread"` **load-bearing** —— 生产 `#[tokio::main]` 多线程,与 keystone 一致。
//! Phase A `tool.execute(...)` 是同步接口(桥起独立 OS 线程 + runtime 跑 async body),本测
//! 不 `.await` 它;Phase B 起 `execute` 变 async,本测调用加 `.await` —— 同 input 同 output 才
//! 证 async 路径等价。

mod common;

use codeagent::subagent::SubagentTool;
use codeagent::tools::Tool; // trait 在作用域才能调 execute

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subagent_tool_real_spawn_returns_wrapped_reply() {
    // gate:开关 + 真 key 任一不就位 → skip(早 return,pass 但事实 skipped)。
    let Some(_key) = common::guard_e2e_or_skip() else {
        return;
    };

    // bin 显式塞真 codeagent exe(理由见文件 docstring:生产 new() 的 current_exe() 在测试里指
    // 测试运行器二进制不是 codeagent CLI)。
    let bin = std::path::PathBuf::from(env!("CARGO_BIN_EXE_codeagent"));
    let tool = SubagentTool::new_with_bin(bin).expect("建 SubagentTool 应成功");

    // 「用一句话回答:好」:不催 tool_call,模型一轮答完直接退;无副作用无写盘,不污染 cwd。
    let args = r#"{"task":"用一句话回答:好"}"#;

    // 外层 120s timeout 真网络 + 一轮模型,DeepSeek 非流式首 token 可能几十秒;超期 = 真 hang/卡
    // → FAIL(非静默挂)。Phase B:execute 升 async、不经 (c′) 桥,直接 `.await` tokio 子进程 IO
    // —— Phase A 基线经桥绿 + Phase B 无桥再绿 才证 SubagentTool 真 spawn 路径运行期等价。
    let call = async { tool.execute(args).await };
    let out = tokio::time::timeout(std::time::Duration::from_secs(120), call)
        .await
        .expect("SubagentTool::execute 应在 120s 内 resolve;挂/卡 = FAIL")
        .expect("SubagentTool::execute(真 spawn 一轮模型) 应 Ok");

    // SubagentTool::execute 把子进程 stdout 包成「[subagent 答复] ... [/subagent 答复]」回灌。
    assert!(
        out.contains("[subagent 答复]"),
        "subagent 真链应回灌「[subagent 答复]」包裹;实得:\n{out}"
    );
    // 模型答「好」或近义一句 —— 断言终答非空、不含错误包裹(它的错误是 anyhow! 串,不会进
    // [subagent 答复] 体内;若 spawn/IO 失败上面那 .expect 已挂)。
    assert!(
        !out.trim().is_empty(),
        "subagent 终答包裹体应非空;实得:\n{out}"
    );
}
