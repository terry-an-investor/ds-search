//! `meta` — site-agnostic page inspection (scan / click / save / diff / watch / response).

use crate::types::{CmdResult, kimi, split_arg};
use pilot::KimiPrimitives;

pub async fn handle(session: String, arg: String) -> CmdResult {
    let (sub, sub_arg) = split_arg(&arg);
    let k = kimi(&session);

    match sub {
        "scan" => do_meta_scan(&k).await,
        "click" => do_meta_click(&k, sub_arg).await,
        "url" => Ok(k.get_url().await),
        "save" => do_meta_save(&k, sub_arg).await,
        "diff" => do_meta_diff(&k, sub_arg).await,
        "watch" => do_meta_watch(&k, sub_arg).await,
        "response" => do_meta_response(&k, sub_arg).await,
        _ => Err("meta subcommands: scan click url save diff watch response".into()),
    }
}

// ════════════════════════════════════════════════════════
// Meta helper functions
// ════════════════════════════════════════════════════════

async fn do_meta_scan(kimi: &KimiPrimitives) -> CmdResult {
    let (raw, _) = kimi.eval_js(
        r#"JSON.stringify((()=>{
            const vh=window.innerHeight;
            const inputs=Array.from(document.querySelectorAll(
                'textarea, input:not([type=hidden]), [contenteditable=true]'
            )).map(function(e){
                const r=e.getBoundingClientRect();
                const bottomDist=vh-r.bottom;
                const nearBottom=bottomDist<200&&r.top>vh*0.3;
                return {
                    tag:e.tagName, type:e.type||'', placeholder:e.placeholder||'',
                    value:(e.value||e.textContent||'').substring(0,80), disabled:e.disabled,
                    nearBottom:nearBottom, w:Math.round(r.width), h:Math.round(r.height)
                };
            });
            inputs.sort(function(a,b){
                if(a.nearBottom&&!b.nearBottom)return -1;
                if(!a.nearBottom&&b.nearBottom)return 1;
                return (b.w*b.h)-(a.w*a.h);
            });
            const allButtons=Array.from(document.querySelectorAll(
                'button, [role=button], [role=radio], [role=switch], [role=tab]'
            )).map(function(e){return{
                text:(e.textContent||'').trim().replace(/\s+/g,' ').substring(0,60),
                role:e.getAttribute('role')||e.tagName.toLowerCase(),
                checked:e.getAttribute('aria-checked')||e.getAttribute('aria-pressed')||'',
                disabled:e.disabled,
                parentClass:(e.parentElement?.className||'').split(' ').slice(0,3).join(' ')
            }});
            const buttons=allButtons.filter(function(b){return b.text.length>0});
            const spinnerDetails=[
                {selector:'.ds-loading',count:document.querySelectorAll('.ds-loading').length},
                {selector:'[aria-busy=true]',count:document.querySelectorAll('[aria-busy=true]').length},
                {selector:'[class*=spinner]',count:document.querySelectorAll('[class*=spinner]').length},
                {selector:'[class*=loading]',count:document.querySelectorAll('[class*=loading]').length}
            ];
            return {
                url:location.href, title:document.title,
                inputs:inputs, buttons:buttons, totalButtons:allButtons.length,
                spinners:spinnerDetails,
                bodySnippet:document.body?document.body.innerText.substring(0,500):''
            };
        })())"#,
    ).await;

    let v: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
    let mut out = String::new();
    out.push_str(&format!(
        "url:    {}\n",
        v.get("url").and_then(|s| s.as_str()).unwrap_or("")
    ));
    out.push_str(&format!(
        "title:  {}\n",
        v.get("title").and_then(|s| s.as_str()).unwrap_or("")
    ));

    if let Some(spinners) = v.get("spinners").and_then(|a| a.as_array()) {
        let active: Vec<_> = spinners
            .iter()
            .filter(|s| s.get("count").and_then(|c| c.as_u64()).unwrap_or(0) > 0)
            .collect();
        out.push_str(&format!(
            "spinners: {} selectors matched ({} elements total)\n",
            active.len(),
            active
                .iter()
                .map(|s| s.get("count").and_then(|c| c.as_u64()).unwrap_or(0))
                .sum::<u64>()
        ));
        for s in &active {
            out.push_str(&format!(
                "  {} ×{}\n",
                s.get("selector").and_then(|v| v.as_str()).unwrap_or(""),
                s.get("count").and_then(|c| c.as_u64()).unwrap_or(0)
            ));
        }
    }

    if let Some(inputs) = v.get("inputs").and_then(|a| a.as_array()) {
        out.push_str(&format!("inputs ({}):\n", inputs.len()));
        for inp in inputs {
            let near = inp
                .get("nearBottom")
                .and_then(|b| b.as_bool())
                .unwrap_or(false);
            let star = if near { " ★" } else { "" };
            out.push_str(&format!(
                "  [{}] placeholder='{}' disabled={}{}\n",
                inp.get("tag").and_then(|s| s.as_str()).unwrap_or(""),
                inp.get("placeholder")
                    .and_then(|s| s.as_str())
                    .unwrap_or(""),
                inp.get("disabled")
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false),
                star,
            ));
        }
    }
    if let Some(buttons) = v.get("buttons").and_then(|a| a.as_array()) {
        out.push_str(&format!("buttons ({}):\n", buttons.len()));
        for b in buttons {
            out.push_str(&format!(
                "  [{}] '{}' checked={} disabled={}\n",
                b.get("role").and_then(|s| s.as_str()).unwrap_or(""),
                b.get("text").and_then(|s| s.as_str()).unwrap_or(""),
                b.get("checked").and_then(|s| s.as_str()).unwrap_or("-"),
                b.get("disabled").and_then(|b| b.as_bool()).unwrap_or(false),
            ));
        }
    }
    if v.get("totalButtons").and_then(|n| n.as_u64()).unwrap_or(0) > 20 {
        out.push_str("  (many icon-only buttons hidden)\n");
    }
    out.push_str(&format!(
        "body:   {}\n",
        v.get("bodySnippet")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .chars()
            .take(300)
            .collect::<String>()
    ));
    Ok(out)
}

async fn do_meta_click(kimi: &KimiPrimitives, text: &str) -> CmdResult {
    if text.is_empty() {
        return Err("meta click requires text to find".into());
    }
    let (raw, _) = kimi.eval_js(&format!(
        r#"JSON.stringify((()=>{{
            const els=Array.from(document.querySelectorAll(
                'button,[role=button],[role=radio],[role=tab],a,[role=link]'
            ));
            const t=els.find(function(e){{return (e.textContent||'').includes('{}');}});
            if(!t)return {{found:false,samples:els.slice(0,10).map(function(e){{return(e.textContent||'').trim().substring(0,40)}})}};
            t.click();
            return {{found:true,text:(t.textContent||'').trim().substring(0,60),tag:t.tagName,parentClass:(t.parentElement?.className||'').split(' ').slice(0,5).join(' '),href:t.getAttribute('href')||''}};
        }})())"#,
        text
    )).await;
    Ok(raw)
}

async fn do_meta_save(kimi: &KimiPrimitives, name: &str) -> CmdResult {
    if name.is_empty() {
        return Err("meta save requires a name".into());
    }
    let safe_name = name.replace(['/', '\\', '.'], "_");
    let dir = std::path::Path::new("knowledge/scans");
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.json", safe_name));
    let (scan_raw, _) = kimi.eval_js(
        r#"JSON.stringify((()=>{const b=Array.from(document.querySelectorAll('button,[role=button],[role=radio],[role=switch],[role=tab]')).map(function(e){return{text:(e.textContent||'').trim().replace(/\s+/g,' ').substring(0,60),role:e.getAttribute('role')||e.tagName.toLowerCase(),checked:e.getAttribute('aria-checked')||e.getAttribute('aria-pressed')||'',disabled:e.disabled}}).filter(function(b){return b.text.length>0});const i=Array.from(document.querySelectorAll('textarea,input:not([type=hidden]),[contenteditable=true]')).map(function(e){return{tag:e.tagName,type:e.type||'',placeholder:e.placeholder||'',disabled:e.disabled}});const dynEls=Array.from(document.querySelectorAll('[class*=response],[class*=message],[class*=turn],[class*=thought]')).map(function(e){return{cls:e.className.split(' ').slice(0,2).join(' '),len:(e.textContent||'').length}});return JSON.stringify({url:location.href,title:document.title,inputs:i,buttons:b,bodySnippet:(document.body?.innerText||'').substring(0,2000),dynEls:dynEls,timestamp:Date.now()})})())"#,
    ).await;
    let parsed: serde_json::Value =
        serde_json::from_str(&scan_raw).unwrap_or(serde_json::Value::String(scan_raw.clone()));
    let to_save = if parsed.is_string() {
        parsed.as_str().unwrap_or(&scan_raw).to_string()
    } else {
        scan_raw
    };
    let pretty: serde_json::Value = serde_json::from_str(&to_save)?;
    std::fs::write(&path, serde_json::to_string_pretty(&pretty)?)?;
    Ok(format!("saved to {}", path.display()))
}

async fn do_meta_diff(kimi: &KimiPrimitives, name: &str) -> CmdResult {
    if name.is_empty() {
        return Err("meta diff requires a saved snapshot name".into());
    }
    let safe_name = name.replace(['/', '\\', '.'], "_");
    let path = std::path::Path::new("knowledge/scans").join(format!("{}.json", safe_name));
    let old: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&path)
            .map_err(|_| format!("snapshot '{}' not found at {}", name, path.display()))?,
    )?;
    let (new_raw, _) = kimi.eval_js(
        r#"JSON.stringify((()=>{const b=Array.from(document.querySelectorAll('button,[role=button],[role=radio],[role=switch],[role=tab]')).map(function(e){return{text:(e.textContent||'').trim().replace(/\s+/g,' ').substring(0,60),role:e.getAttribute('role')||e.tagName.toLowerCase(),checked:e.getAttribute('aria-checked')||e.getAttribute('aria-pressed')||'',disabled:e.disabled}}).filter(function(b){return b.text.length>0});const i=Array.from(document.querySelectorAll('textarea,input:not([type=hidden]),[contenteditable=true]')).map(function(e){return{tag:e.tagName,type:e.type||'',placeholder:e.placeholder||'',disabled:e.disabled}});const dynEls=Array.from(document.querySelectorAll('[class*=response],[class*=message],[class*=turn],[class*=thought]')).map(function(e){return{cls:e.className.split(' ').slice(0,2).join(' '),len:(e.textContent||'').length}});return JSON.stringify({url:location.href,title:document.title,inputs:i,buttons:b,bodySnippet:(document.body?.innerText||'').substring(0,2000),dynEls:dynEls,timestamp:Date.now()})})())"#,
    ).await;
    let new_parsed: serde_json::Value =
        serde_json::from_str(&new_raw).unwrap_or(serde_json::Value::String(new_raw.clone()));
    let new_str = if new_parsed.is_string() {
        new_parsed.as_str().unwrap_or(&new_raw).to_string()
    } else {
        new_raw
    };
    let new: serde_json::Value = serde_json::from_str(&new_str)?;
    let mut out = String::new();
    out.push_str(&format!("diff '{}' vs live:\n", name));
    let old_url = old.get("url").and_then(|s| s.as_str()).unwrap_or("");
    let new_url = new.get("url").and_then(|s| s.as_str()).unwrap_or("");
    let old_btns = old
        .get("buttons")
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default();
    let new_btns = new
        .get("buttons")
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default();
    let old_set: std::collections::HashSet<String> = old_btns
        .iter()
        .filter_map(|b| {
            b.get("text")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    let new_set: std::collections::HashSet<String> = new_btns
        .iter()
        .filter_map(|b| {
            b.get("text")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    let added: Vec<_> = new_set.difference(&old_set).collect();
    let removed: Vec<_> = old_set.difference(&new_set).collect();
    let old_dyn = old
        .get("dynEls")
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default();
    let new_dyn = new
        .get("dynEls")
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default();
    let old_body = old
        .get("bodySnippet")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    let new_body = new
        .get("bodySnippet")
        .and_then(|s| s.as_str())
        .unwrap_or("");

    let mut changes = 0;
    if old_url != new_url {
        out.push_str(&format!("  URL: {} → {}\n", old_url, new_url));
        changes += 1;
    }
    if !added.is_empty() {
        out.push_str(&format!(
            "  + added: {}\n",
            added
                .iter()
                .map(|s| format!("'{}'", s))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        changes += 1;
    }
    if !removed.is_empty() {
        out.push_str(&format!(
            "  - removed: {}\n",
            removed
                .iter()
                .map(|s| format!("'{}'", s))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        changes += 1;
    }
    for nb in &new_btns {
        let nt = nb.get("text").and_then(|s| s.as_str()).unwrap_or("");
        let nc = nb.get("checked").and_then(|s| s.as_str()).unwrap_or("");
        if let Some(ob) = old_btns
            .iter()
            .find(|b| b.get("text").and_then(|s| s.as_str()) == Some(nt))
        {
            let oc = ob.get("checked").and_then(|s| s.as_str()).unwrap_or("");
            if oc != nc && !oc.is_empty() {
                out.push_str(&format!("  ~ '{}' checked: {} → {}\n", nt, oc, nc));
                changes += 1;
            }
        }
    }
    if new_dyn.len() != old_dyn.len() {
        out.push_str(&format!(
            "  Δ dynamic elements: {} → {}\n",
            old_dyn.len(),
            new_dyn.len()
        ));
        changes += 1;
        for nd in &new_dyn {
            let nc = nd.get("cls").and_then(|s| s.as_str()).unwrap_or("");
            let nl = nd.get("len").and_then(|n| n.as_u64()).unwrap_or(0);
            let found = old_dyn.iter().any(|od| {
                od.get("cls").and_then(|s| s.as_str()) == Some(nc)
                    && od.get("len").and_then(|n| n.as_u64()) == Some(nl)
            });
            if !found && nl > 0 {
                out.push_str(&format!("    new: {} ({} chars)\n", nc, nl));
            }
        }
    }
    if old_body != new_body {
        let old_words: std::collections::HashSet<&str> = old_body.split(' ').collect();
        let new_words: std::collections::HashSet<&str> = new_body.split(' ').collect();
        let new_wc: Vec<_> = new_words.difference(&old_words).collect();
        if !new_wc.is_empty() && new_wc.len() < 30 {
            out.push_str(&format!("  Δ body: +{} new words\n", new_wc.len()));
            changes += 1;
        } else if new_body.len() > old_body.len() + 100 {
            out.push_str(&format!(
                "  Δ body: {} → {} chars (+{})\n",
                old_body.len(),
                new_body.len(),
                new_body.len() - old_body.len()
            ));
            changes += 1;
        }
    }
    if changes == 0 {
        out.push_str("  (no changes detected)\n");
    }
    Ok(out)
}

async fn do_meta_watch(kimi: &KimiPrimitives, arg: &str) -> CmdResult {
    let interval_ms: u64 = arg.parse().unwrap_or(1000);
    let rounds: u64 = if arg.contains('x') {
        arg.split('x')
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(10)
    } else {
        10
    };

    let mut out = String::new();
    let mut last_body = String::new();
    let mut last_url = String::new();

    for i in 0..rounds {
        tokio::time::sleep(std::time::Duration::from_millis(interval_ms)).await;
        let (url, _) = kimi.eval_js("location.href").await;
        let (body, _) = kimi
            .eval_js("document.body?.innerText?.substring(0,300) || ''")
            .await;

        let mut changes = Vec::new();
        if url != last_url && !last_url.is_empty() {
            changes.push(format!("URL: {} → {}", last_url, url));
        }
        if body != last_body && !last_body.is_empty() {
            let delta = body.len() as i64 - last_body.len() as i64;
            if delta != 0 {
                changes.push(format!(
                    "body: {}{} chars",
                    if delta > 0 { "+" } else { "" },
                    delta
                ));
            }
        }

        if !changes.is_empty() || i == 0 {
            out.push_str(&format!(
                "[{}ms] {}\n",
                i * interval_ms,
                changes.join(" | ")
            ));
        }
        if i == 0 || !changes.is_empty() {
            out.push_str(&format!(
                "  url={}\n  body={}\n",
                &url[..url.len().min(80)],
                &body[..body.len().min(120)]
            ));
        }

        last_url = url;
        last_body = body;
    }
    Ok(out)
}

async fn do_meta_response(kimi: &KimiPrimitives, _arg: &str) -> CmdResult {
    let (before, _) = kimi
        .eval_js("document.body?.innerText?.substring(0,3000) || ''")
        .await;
    let before_count = before.len();

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut last_len = before_count;
    let mut stable = 0;
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let (body, _) = kimi
            .eval_js("document.body?.innerText?.substring(0,5000) || ''")
            .await;
        let len = body.len();
        if len == last_len {
            stable += 1;
            if stable >= 4 && len > before_count {
                break;
            }
        } else {
            last_len = len;
            stable = 0;
        }
        if tokio::time::Instant::now() > deadline {
            break;
        }
    }

    let (after, _) = kimi
        .eval_js("document.body?.innerText?.substring(0,5000) || ''")
        .await;
    let mut out = String::new();
    out.push_str(&format!(
        "body: {} → {} chars (+{})\n",
        before_count,
        after.len(),
        after.len().saturating_sub(before_count)
    ));

    if after.len() > before_count + 20 {
        let new_text = &after[before_count.min(after.len())..];
        out.push_str(&format!(
            "new content:\n---\n{}\n---\n",
            &new_text[..new_text.len().min(1000)]
        ));
    } else if after.len() <= before_count {
        out.push_str("(no new content detected)\n");
    }
    Ok(out)
}
