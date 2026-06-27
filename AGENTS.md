# Agent 第一原则：观察 → 行动 → 验证

## 核心铁律

**永远不要相信任何操作的返回值。只相信操作后重新从 DOM 读回来的状态。**

```
evaluate 返回 ok:true   ≠   操作真的生效了
fill 返回 success       ≠   文字真的出现在输入框里
click 返回 success      ≠   UI 真的响应了
send_message 返回 Ok(()) ≠  消息真的发送出去了
```

## 三步循环

每一步交互必须走完这个闭环：

```
┌─ 1. OBSERVE ──────────────────────────────────┐
│  读当前 DOM 状态                                │
│  - 输入框的 value / innerText                  │
│  - 按钮的 aria 属性 / disabled 状态             │
│  - 页面上可见的关键文字                          │
│  - URL 是否包含预期参数                          │
│  - spinner / loading 是否存在                   │
│  - 消息数量是否变化                              │
├─ 2. ACT ───────────────────────────────────────┤
│  执行一个操作（fill / key_type / click / send）    │
├─ 3. VERIFY ────────────────────────────────────┤
│  重新读 DOM 状态                                │
│  - 对比 OBSERVE 阶段的值                        │
│  - 确认状态确实改变了                            │
│  - 如果没变 → 换策略，不要重复同一个方法          │
└────────────────────────────────────────────────┘
```

## 输入文字的降级链

```
1. 先试 fill     → verify: 读 textarea.value，确认文字存在
2. 失败换 key_type → verify: 再读 textarea.value
3. 还不行：dump 输入框的所有属性（tagName, contenteditable, 父元素结构）
4. 绝不能：填完不管，直接进入下一步
```

## 提交的降级链

```
1. 先试 send_keys "Enter" → verify: URL 变了？DOM 里出现新消息了？
2. 失败换 click 发送按钮    → verify: 同上
3. 都失败：dump 发送按钮的属性、父级 form、附近所有按钮的文本
```

## 验证的具体方法

不是看返回值，是看这些：

| 操作 | 验证方法 |
|------|---------|
| 输入文字 | `evaluate` 读 `textarea.value.length` |
| 点击按钮 | `evaluate` 读 URL 是否变化、目标元素是否出现 |
| 发送消息 | 读 `message_count`、查 spinning 状态 |
| 切换模式 | 读对应 radio 的 `aria-checked` |
| 等待响应 | 持续 poll `is_streaming`，直到停止 + 稳定 3 轮 |
| 提取响应 | 先 `scroll_virtual_list`，再读 `.ds-markdown`，确认长度 > 0 |
| 安装日志 | 立即触发一条 console.log 验证 `window.__dsLog` 有数据 |

## 失败时的 debug dump

当任何步骤验证失败，立即执行以下诊断：

```js
// 1. 当前 URL
window.location.href

// 2. 所有 textarea 状态
document.querySelectorAll('textarea').length + 每个的 placeholder/value

// 3. 所有 role="radio" 元素
Array.from(document.querySelectorAll('[role="radio"]')).map(r => ({
  text: r.textContent.trim(),
  checked: r.getAttribute('aria-checked'),
  parent: r.parentElement?.tagName
}))

// 4. 页面关键文本
document.body.innerText.substring(0, 500)

// 5. 任何异常提示
document.querySelector('[role="alert"], .error, .warning')?.textContent

// 6. Spinner / loading 状态
document.querySelector('.ds-loading, [aria-busy="true"]')
```

## 具体案例：DeepSeek send_message 的完整 debug 流

```rust
// OBSERVE — 发送前状态
let before_url = kimi.get_url().await;
let before_count = sem.get_fast_state().await.message_count;
let (ta_val, _) = kimi.eval_js("document.querySelector('textarea')?.value || ''").await;
println!("  [OBSERVE] url={} messages={} textarea='{}'", before_url, before_count, ta_val);

// ACT
sem.send_message("hello").await?;

// VERIFY — 发送后状态
tokio::time::sleep(Duration::from_millis(500)).await;
let after_url = kimi.get_url().await;
let after_count = sem.get_fast_state().await.message_count;
let (ta_val2, _) = kimi.eval_js("document.querySelector('textarea')?.value || ''").await;
println!("  [VERIFY] url={} messages={} textarea='{}'", after_url, after_count, ta_val2);

// 验证清单
assert!(ta_val2.is_empty(), "发送后输入框应该被清空");  // textarea 清空了
assert!(after_url.contains("chat/") || after_count > before_count, "URL 或消息数应该变化");
```

## 禁止事项

1. **禁止连续两次执行同一个失败的操作。** 第一次 `fill` 失败 → 立刻换 `key_type`，不要再试 `fill`。
2. **禁止不验证就进入下一步。** 发了消息不确认 → 后面全乱套。
3. **禁止相信 `ok: true`。** 那是 HTTP 层的成功，不是业务层的成功。
4. **禁止在验证失败时忽略问题继续跑。** 一步失败 = 后面的测试没有意义。
5. **禁止用"看起来差不多"代替精确验证。** `message_count` 没变 = 就是没变，没理由。
6. **禁止用错误的探针 VERIFY。** VERIFY 用错选择器 = 没验证，甚至比不验证更危险（得出错误结论）。不确定结构时，先 OBSERVE 真实 DOM 再决定探针。

## 代码层强制（pilot::verify）

上面是纪律。纪律会被忘。所以本仓库把 VERIFY 编进了代码：

- `pilot/src/verify.rs` 提供 `VerifyDriven` trait（`fill_and_verify` / `act_and_verify`）。
- 每个 ACT 方法内部强制带 VERIFY：填文字→读 `textarea.value`；点按钮→读 turn 数/URL。
- VERIFY 不过返回 `AdapterError::VerifyFailed`，带 before/after diff —— **绝不静默返回 `Ok(())`**。

**写 adapter 时优先用 `VerifyDriven`，不要手写 fill-then-hope 模式。** 详见 skill `verify-driven-automation`。

### 每次 ACT 后必走的 verify checklist

| 操作 | 必须读回的 DOM 探针 | 错误探针（别犯） |
|------|-------------------|----------------|
| fill 文字 | `textarea.value === expected` | 只看返回值 ok |
| click Run | `.chat-turn-container.user` 数量增加 | 只看 clicked |
| toggle 工具 | `.mdc-switch--checked` 类 | 找不存在的 `aria-pressed` / radio |
| select 模型 | `.model-selector-card .title` 文本 | 读整个卡片（混入 ID） |
| 设系统指令 | 读回 textarea.value 比对 | 填完不读 |
| 设 thinking | `.mat-mdc-select-min-line`（是 select） | 找 `role=radio` |
| 等待响应 | run-time pill（`.model-run-time-pill`） | `[class*=loading]`（含被动 placeholder） |

## 历史反面案例（真实踩坑，别重蹈）

1. **无条件 Escape 清空输入框** — `dismiss_dialogs` 返回 ok，但末尾无条件发 Escape，在 textarea 聚焦时清空内容 → 后续所有 send "not accepted"。返回值完全正常。**教训：返回值会撒谎，DOM 不会。**
2. **fill 不 verify** — `set_prompt_text` 返回 Ok，但 Angular 表单没收到值 → 点 Run 空转。**教训：fill 后必须读 `textarea.value`。**
3. **VERIFY 选择器错** — toggle 工具后找 `button[aria-pressed]`，但实际是 `mat-slide-toggle` + `.mdc-switch--checked` 类 → 读回 null，误判"没生效"。**教训：VERIFY 必须用正确的探针。**
4. **填错框** — 没 OBSERVE 页面有几个输入框，把系统指令填进了 prompt 框。**教训：ACT 前先 OBSERVE 现状。**

## 总结

```
                     别信返回值
                        │
              ┌─────────┼─────────┐
              ▼         ▼         ▼
          fill成功   click成功   send成功
              │         │         │
              └─────────┼─────────┘
                        ▼
               "我应该看看输入框"
                        │
              ┌─────────┼─────────┐
              ▼         ▼         ▼
          value为空  URL没变   没新消息
                        │
                        ▼
              "操，根本没生效"
```

**Agent 不是脚本。Agent 是观察者 + 决策者。脚本只会顺序执行然后报 timeout；Agent 每一步都看，看完了再决定下一步干什么。**
