import Link from "next/link";
import { buildPageMetadata } from "@/lib/page-meta";

export async function generateMetadata({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  const isZh = locale === "zh";
  return buildPageMetadata({
    path: "/docs/tools",
    locale,
    title: isZh ? "工具 · Codewhale 文档" : "Tools · Codewhale Docs",
    description: isZh
      ? "Bash、File、Git、Run 四个核心 action 工具，以及协调、延迟加载与回放兼容边界。"
      : "Canonical Bash, File, Git, and Run action tools, coordination tools, deferred loading, and replay compatibility.",
  });
}

export default async function ToolsPage({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  const isZh = locale === "zh";

  return (
    <section className="space-y-10">
      <section id="overview" className="scroll-mt-32">
        <h2 className="font-display text-3xl mb-1">
          {isZh ? "工具" : "Tools"}{" "}
          <span className="font-cjk text-indigo text-2xl ml-2">
            {isZh ? "Tools" : "工具"}
          </span>
        </h2>
        <p className={`text-ink-soft mt-3 ${isZh ? "leading-[1.9] tracking-wide" : "leading-relaxed"}`}>
          {isZh
            ? "精选工具集——设计思路详见 "
            : "Curated surface — see "}
          <Link
            href="https://github.com/Hmbown/CodeWhale/blob/main/docs/TOOL_SURFACE.md"
            className="body-link"
          >
            docs/TOOL_SURFACE.md
          </Link>
          {isZh ? "。" : " for design rationale."}
        </p>
        <div className="hairline-t hairline-b mt-6">
          {[
            {
              group: "Bash",
              tools: "run · wait · interact · cancel",
            },
            {
              group: "File",
              tools: "read · list · search_name · search_content · write · edit · patch",
            },
            {
              group: "Git",
              tools: "status · diff · log · show · blame",
            },
            {
              group: "Run",
              tools: "tests · verifiers",
            },
            {
              group: isZh ? "协调" : "Coordination",
              tools: isZh
                ? "agent · tasks · update_plan · work_update · tool_search"
                : "agent · tasks · update_plan · work_update · tool_search",
            },
            {
              group: isZh ? "延迟加载" : "Deferred",
              tools: isZh
                ? "Web（search · fetch · wait）仅在网络策略允许时通过 tool_search 加载；github、automation 与 rlm 也默认延迟加载"
                : "Web (search · fetch · wait) loads through tool_search only when network policy permits; github, automation, and rlm are deferred by default",
            },
            {
              group: "MCP",
              tools: isZh
                ? "mcp_<server>_<tool>——从 ~/.codewhale/mcp.json 自动注册"
                : "mcp_<server>_<tool> — auto-registered from ~/.codewhale/mcp.json",
            },
          ].map((row) => (
            <div
              key={row.group}
              className="grid md:grid-cols-12 gap-0 hairline-t py-3 px-4 hover:bg-paper-deep transition-colors min-w-0"
            >
              <div className="md:col-span-3 font-display text-sm font-semibold">
                {row.group}
              </div>
              <div className="md:col-span-9 font-mono text-[0.78rem] text-ink-soft leading-relaxed break-words min-w-0">
                {row.tools}
              </div>
            </div>
          ))}
        </div>
      </section>

      <section id="compatibility" className="scroll-mt-32">
        <h2 className="font-display text-2xl mb-1">
          {isZh ? "回放兼容" : "Replay compatibility"}
        </h2>
        <p className={`text-ink-soft mt-3 ${isZh ? "leading-[1.9] tracking-wide" : "leading-relaxed"}`}>
          {isZh
            ? "旧的单用途工具名只为已保存的 transcript 与自动化回放保留。它们仍可按原名执行，但不会出现在模型目录或 tool_search 中；新工作应使用上方的 canonical action 工具。"
            : "Legacy single-purpose names remain callable only so saved transcripts and automation can replay. They stay out of the model catalog and tool_search; new work uses the canonical action tools above."}
        </p>
        <Link
          href="https://github.com/Hmbown/CodeWhale/blob/main/docs/RUNTIME_SIMPLIFICATION_DESIGN.md"
          className="inline-block mt-3 font-mono text-xs uppercase tracking-wider text-indigo hover:underline"
        >
          docs/RUNTIME_SIMPLIFICATION_DESIGN.md →
        </Link>
      </section>

      <section id="source" className="hairline-t pt-8">
        <p className="text-sm text-ink-mute">
          {isZh
            ? "来源文档：docs/TOOL_SURFACE.md, docs/RUNTIME_SIMPLIFICATION_DESIGN.md · 更新时请同步修改 docs-map.ts。"
            : "Source documents: docs/TOOL_SURFACE.md, docs/RUNTIME_SIMPLIFICATION_DESIGN.md · Update docs-map.ts when changing."}
        </p>
      </section>
    </section>
  );
}
