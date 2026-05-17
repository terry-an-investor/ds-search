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
