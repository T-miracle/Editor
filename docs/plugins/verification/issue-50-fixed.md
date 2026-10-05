## 更正：先前报告的那条失败用例已修复，而根因不在产品

本工单下我曾报告「属于本工单范围的『准备期间停止』有一条验收用例稳定失败」，并据此建议重新评估关闭状态。**那条报告现在需要更正，因为它会把读者引向错误的结论。**

### 现在的结果

```
cargo test -p editor-app --bin editor-app extensions::native_build_tests -- --ignored --test-threads=1
test result: ok. 5 passed; 0 failed; 0 ignored

cargo test -p editor-app --bin editor-app extensions::native_run_tests -- --ignored --test-threads=1
test result: ok. 7 passed; 0 failed; 0 ignored
```

**`stopping_during_preparation_never_starts_the_program` 通过**，`leaving_stops_every_run_session_before_shutdown` 也一并通过。

### 真正的根因：用例把需要执行的工作项自己取走了

失败**不是**产品缺陷，而是**用例自身的读取方式**：

1. 用例在编辑器发出停止请求后，用 **`Receiver::try_iter` 读工作队列**——**该调用会排空通道**；
2. 它随即**又自己调用 `manager.stop_execution(session)`**；
3. 于是那个 **`Work::StopRun` 在夹具（本应在 `frame` 内按生产路径执行它）拿到之前就被取走**；
4. **`stop_results` 永远为空 → `note_preparation_stopped` 永不运行 → 序列永不阻塞 → 用例超时。**

**症状出现在下游，看起来像产品缺陷** —— 这也是我先前那份报告出错的原因。

### 修法

用例**不再读队列、也不再自己执行停止**，只等待准备被报告为已阻塞，并断言它真正要测的两件事：**准备给出的原因含「停止」**，以及**程序从未被启动**。

**一处断言被有意收窄并写明理由**：运行时自己的会话状态在提供者被要求停止后**仍是 `Running`**——因为**该提供者在「终止已发出」时就应答停止**，而**父设计第 47 行正是要求「受理不等于结束」**。**在那里断言会话已结束会与设计相矛盾**，因此用例写明了为什么不断言它。

### 通道逐环实测（修复后）

```
probe harness_stop session=1 request=4 ok=true        ← 夹具确实执行了该请求
probe reconcile published=[("<cfg>", 4, Ok(()))]      ← 结果确实到达编辑器
```

**四个环节现在都有实测**，不再有猜测。

### 对本工单处置的影响

**先前的建议（重新打开并做设计改动）不再成立。** 本工单其余判据的证据未受影响，且这次修复去掉了一条被误判为产品缺陷的失败项。**是否重新打开，请按其余判据（环境受限项）判断，而不是按这条已修复的用例。**

### 我应当更早发现的一点

我在这条用例上做了**十四次产品侧改动，全部无效**。**转折点是停止改代码、改为要求「同一时刻的证据」**——一次同刻打印就显示出：停止请求发出并被登记，但应答从未产生。**排空式读取（`try_iter`／`try_recv`）会让另一个消费者永远拿不到数据，而症状出现在下游。** 这一点已记入内部验收文档，供后续参考。
