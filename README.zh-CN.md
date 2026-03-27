# vmrp-rust

`vmrp-rust` 是 VMRP 运行时的 Rust 重写版本，目标是运行历史 `.mrp` 包。

## 当前状态

- `asm.mrp` 的引导链路已可执行。
- 启动流程已打通：`mr_c_function_load -> extHelper(code=0) -> DSM_INIT -> MR_START_DSM`。
- `vmrp-windows` 现在有明确退出码：
  - `0`：引导运行成功
  - 非 `0`：引导运行失败

最近一次验证命令：

```powershell
cargo run -p vmrp-windows --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml -- D:\opt\rust\vmrp\mrc\asm\asm.mrp
```

预期关键输出：

```text
mrp_bootstrap_run_ok=true
```

## 构建与测试

```powershell
cargo test --workspace --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml
```

## 运行方式

```powershell
cargo run -p vmrp-windows --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml -- [path-to.mrp]
```

可选参数：

- `--verbose` / `-v`：打印执行跟踪日志
- `--step-limit N`：每个阶段的最大执行步数
- `--trace-limit N`：每个阶段最多输出多少条跟踪日志

## 兼容性说明

当前实现仍处于分阶段兼容构建：

- 已验证样例的核心引导与事件入口可运行。
- `DSM_REQUIRE_FUNCS` 已接入可用的基础 Host 映射（文件/时间/内存/日志 + 最小桩函数）。
- 对历史 `.mrp` 的全面兼容 **尚未完成**。

## 下一步目标

1. 扩充真实 `.mrp` 回归样本集。
2. 完成高频 DSM API（网络/音频/UI 路径）。
3. 补全更完整的运行时事件循环集成。

## 参考

本实现参考了以下项目：

- https://github.com/vmrp/vmrp
