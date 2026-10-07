## 本工单范围内的一条验收用例目前失败——建议重新评估关闭状态

本工单已关闭，但在交付前的逐模块串行复核中发现：**属于本工单范围的准备期间停止，有一条验收用例稳定失败**。

### 失败用例

```
cargo test -p editor-app --bin editor-app stopping_during_preparation -- --ignored --test-threads=1
extensions::native_build_tests::stopping_during_preparation_never_starts_the_program ... FAILED
```

**稳定失败**（连续两次运行逐条比对，用例集合相同：5 条、4 通过、1 失败），失败点是 `preparation_blocked` 始终为 `None`。

### 已定位的机制（实测）

```
probe A step=0 current=Some(1)      // 步骤在跑，用例调用 stop_preparations()
probe B step=1 requested=Some(1)    // 序列确实发出了停止请求
probe C step=2..9 blocked=None      // 之后序列永不阻塞
```

`manager.stop_execution` 之后，**编辑器侧的 `RunSession.state` 从未变为非活动**，而 `preparation_blocked` 要求 `!is_active()`。

### 引入点

测试二分（串行）：`d475b89` 通过，**`7c5cfcd` 起失败**。该提交为「同一次停止被反复重发」加了一个守卫：

```rust
if self.run_controls.is_stopping(config) { return; }
```

**它按「配置」判断，而一次准备步骤与它随后启动的程序可以属于同一个配置**——用户先点「停止」后，准备序列自己那次真正关键的停止请求被吞掉。

### 为什么关闭状态值得重新评估

父设计 `docs/run-debug-build.md` 第 47 行要求「准备期间停止：先停当前步骤自己拥有的那个会话，并在其结束前保持序列仍被占用」。**这正是失败用例所测的行为，而当前实现做不到**：守卫使序列既不发停止、也不阻塞。

### 已尝试且失败的修法（共八次，全部已撤回）

按「会话」守卫；让标题栏的停止交给序列；在 `reconcile` 里补上「会话已结束也告知序列」；在序列上以「停止是否真的发出」为键；以及上述几种的组合。

**八次都未能让 `stopping_during_preparation…` 与 `leaving_stops…` 同时通过**，因此全部撤回，仓库保持在已验证状态。

### 根因判断

**「停止」在代码里被三个键各记了一次**——配置（`stops` 按 `config` 查）、会话（`stops[].session`）、请求身份（`request_id`）。**要让一次准备步骤的停止有单一发起者与确认者，需要先把这三个键合并**；**那是设计改动，不是缺陷修补**。

### 维护者可选的处置

1. **重新打开本工单**，把「合并停止的三个键」作为一次设计改动实施；或
2. **明确接受该用例所测的行为不在本工单范围内**（若如此，请指出应由哪张工单承担），并据此调整用例或设计文档。

**在处置之前，本工单的关闭状态与「准备期间停止」这条判据不一致**，因此在此如实报告，而不是留到合并后才发现。

### 未受影响的证据

本工单其余判据均有证据：`host_controls_stop_the_program_a_session_owns`（停止经提供者会话完成）、离开确认（`extensions::native_run_tests` **7/7 通过**，含 `leaving_stops_every_run_session_before_shutdown`）、取消留在原处。**失败只集中在「准备步骤的停止」这一条。**
