#!/usr/bin/env python3
"""Exportiert das Session-Transkript (jsonl) als lesbares Markdown-Chat-Log.
Dialog (User-Text + Assistant-Prosa) + kompakte Tool-Aktionszeilen; grosse
Tool-Outputs, System-Reminder und Task-Notifications werden weggelassen."""
import json, sys, re

F = "/home/librechat/.claude/projects/-home-librechat-enginemartuni/ec32cb8c-3fb5-4ae3-a36b-164c09d274dd.jsonl"
OUT = "/home/librechat/enginemartuni/chat_05.06.2026.md"

def text_blocks(content):
    """Yield ('text'|'tool', payload) from a message content (str or list)."""
    if isinstance(content, str):
        if content.strip():
            yield ("text", content)
        return
    if not isinstance(content, list):
        return
    for b in content:
        if not isinstance(b, dict):
            continue
        t = b.get("type")
        if t == "text" and b.get("text", "").strip():
            yield ("text", b["text"])
        elif t == "tool_use":
            yield ("tool", b)
        elif t == "tool_result":
            # nur AskUserQuestion-Antworten (Entscheidungen) durchlassen
            c = b.get("content")
            txt = c if isinstance(c, str) else " ".join(
                x.get("text", "") for x in c if isinstance(x, dict)) if isinstance(c, list) else ""
            if "have been answered" in txt:
                yield ("answer", txt)
        # andere tool_result / thinking / images: skip

def is_noise(txt):
    s = txt.lstrip()
    return (s.startswith("<system-reminder>")
            or s.startswith("<task-notification>")
            or s.startswith("[SYSTEM NOTIFICATION")
            or s.startswith("Caveat:")
            or s.startswith("<command-name>")
            or s.startswith("<local-command"))

def strip_reminders(txt):
    # entferne eingebettete <system-reminder>...</system-reminder> Bloecke
    return re.sub(r"<system-reminder>.*?</system-reminder>", "", txt, flags=re.S).strip()

def tool_line(b):
    name = b.get("name", "?")
    inp = b.get("input", {}) or {}
    desc = inp.get("description") or inp.get("command") or inp.get("prompt") \
        or inp.get("file_path") or inp.get("question") or ""
    if isinstance(desc, str):
        desc = desc.replace("\n", " ")[:140]
    # AskUserQuestion: zeige die Frage
    if name == "AskUserQuestion":
        qs = inp.get("questions", [])
        if qs:
            desc = qs[0].get("question", "")[:160]
    return f"  - 🔧 *{name}*: {desc}".rstrip()

lines = ["# Martuni — Session-Chat 05.06.2026",
         "",
         "*Export des Claude-Code-Verlaufs (Dialog + kompakte Aktionen; Tool-Outputs ausgelassen).*",
         ""]

n_user = n_asst = 0
with open(F) as f:
    for line in f:
        try:
            o = json.loads(line)
        except Exception:
            continue
        typ = o.get("type")
        if typ not in ("user", "assistant"):
            continue
        msg = o.get("message") or {}
        content = msg.get("content")
        blocks = list(text_blocks(content))
        if not blocks:
            continue
        if typ == "user":
            texts = [strip_reminders(p) for k, p in blocks if k == "text"]
            texts = [t for t in texts if t and not is_noise(t)]
            answers = [p for k, p in blocks if k == "answer"]
            if not texts and not answers:
                continue
            n_user += 1
            if texts:
                lines.append(f"\n## 👤 Tobias\n")
                for t in texts:
                    lines.append(t)
            for a in answers:
                # extrahiere die Frage/Antwort kompakt
                m = re.search(r'answered:\s*(.*)', a, re.S)
                ans = (m.group(1).strip() if m else a.strip())
                ans = re.sub(r'\s*You can now continue.*$', '', ans, flags=re.S).strip()
                lines.append(f"\n## ✅ Tobias entscheidet\n")
                lines.append(ans)
        else:  # assistant
            out = []
            for k, p in blocks:
                if k == "text":
                    s = p.strip()
                    if s:
                        out.append(s)
                else:
                    out.append(tool_line(p))
            if not any(not x.startswith("  - 🔧") for x in out):
                # nur Tool-Aktionen, keine Prosa → kompakt anhaengen
                if out:
                    lines.append("\n".join(out))
                continue
            n_asst += 1
            lines.append(f"\n## 🤖 Claude\n")
            lines.append("\n".join(out))

open(OUT, "w").write("\n".join(lines) + "\n")
print(f"user turns: {n_user}, assistant turns: {n_asst}")
print(f"wrote {OUT} ({len(open(OUT).read())} bytes)")
