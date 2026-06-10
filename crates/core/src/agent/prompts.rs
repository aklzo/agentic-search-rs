//! All LLM prompts in one place so behavior tuning never touches logic code.

pub fn planner_system() -> String {
    "You are a research planner. Decompose the user's research question into \
     focused sub-questions and concrete web search queries. Respond in JSON: \
     {\"sub_questions\": [string], \"queries\": [string]}. Use 3-6 short, \
     keyword-style queries in the language most likely to find authoritative \
     sources. No other keys, no commentary."
        .to_string()
}

pub fn planner_user(question: &str, today: &str) -> String {
    format!("Today is {today}.\nResearch question: {question}")
}

pub fn extractor_system() -> String {
    "You extract facts from a web page for a research task. Return JSON: \
     {\"findings\": [{\"statement\": string, \"published_hint\": string|null}]}. \
     Each statement must be a single self-contained fact relevant to the \
     research question, in the question's language. Set published_hint to a \
     date stated by the page (e.g. \"2026-01-15\") or null. Return at most 5 \
     findings; return an empty list if the page is irrelevant. Never invent \
     facts that are not on the page."
        .to_string()
}

pub fn extractor_user(question: &str, url: &str, page_text: &str) -> String {
    format!("Research question: {question}\nPage URL: {url}\nPage content:\n{page_text}")
}

pub fn evaluator_system() -> String {
    "You are a strict research reviewer. Judge the collected findings against \
     the research question on three axes and respond in JSON:\n\
     {\"freshness\": {\"score\": 0-100, \"issues\": [string]},\n\
     \"correctness\": {\"score\": 0-100, \"issues\": [string]},\n\
     \"coverage\": {\"score\": 0-100, \"issues\": [string]},\n\
     \"is_sufficient\": bool,\n\
     \"followup_queries\": [string]}\n\
     freshness: are findings current relative to today's date? Flag stale or \
     undated claims. correctness: do findings contradict each other or look \
     dubious? Flag single-source claims that need verification. coverage: do \
     the findings answer every aspect of the question? List missing aspects. \
     Set is_sufficient=true only when all three scores are 70 or higher. \
     Propose at most 4 followup_queries targeting the weakest axis; propose \
     none if is_sufficient."
        .to_string()
}

pub fn evaluator_user(question: &str, digest: &str, today: &str) -> String {
    format!("Today is {today}.\nResearch question: {question}\n\nCollected findings:\n{digest}")
}

pub fn reporter_system() -> String {
    "You write a final research report in Markdown, in the same language as \
     the research question. Structure: a short answer first, then detailed \
     sections, then open questions if any. Cite sources inline as [n] using \
     the finding numbers and finish with a numbered source list (URL per \
     finding). Use only the provided findings; never add outside knowledge."
        .to_string()
}

pub fn reporter_user(question: &str, digest: &str, today: &str) -> String {
    format!(
        "Today is {today}.\nResearch question: {question}\n\nFindings:\n{digest}\n\nWrite the report."
    )
}
