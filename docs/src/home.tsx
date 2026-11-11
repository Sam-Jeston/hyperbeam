import { ArrowRight, Boxes, Rocket, Server } from "lucide-react";
import type { ReactNode } from "react";
import { href } from "./markdown";
import { NAV } from "./nav";
import type { PageMeta } from "./layout";

export function Home({ pages }: { pages: Record<string, PageMeta> }) {
  const start: { slug: string; icon: ReactNode; cta: string }[] = [
    { slug: "quickstart", icon: <Rocket />, cta: "Run it locally" },
    { slug: "concepts", icon: <Boxes />, cta: "Learn the model" },
    { slug: "install-kubernetes", icon: <Server />, cta: "Install for a team" },
  ];
  return (
    <div className="grid gap-14">
      <div className="grid gap-3 sm:grid-cols-3">
        {start.map(({ slug, icon, cta }) => (
          <a key={slug} href={href(`/${slug}`)} className="group grid content-start gap-3 rounded-xl border border-border bg-surface p-5 transition-[border-color,box-shadow] hover:border-primary/60 hover:shadow-soft">
            <div className="grid size-9 place-items-center rounded-lg bg-accent text-accent-foreground [&_svg]:size-[18px]">{icon}</div>
            <div className="font-display text-[16px] font-semibold tracking-[-0.01em]">{pages[slug].title}</div>
            <p className="text-[13.5px] leading-relaxed text-muted">{pages[slug].description}</p>
            <span className="mt-1 inline-flex items-center gap-1 text-[13.5px] font-medium text-primary">{cta} <ArrowRight className="size-3.5 transition-transform group-hover:translate-x-0.5" /></span>
          </a>
        ))}
      </div>

      <div className="grid gap-10 sm:grid-cols-2">
        {NAV.map((group) => (
          <section key={group.title}>
            <h2 className="mb-3 font-display text-[17px] font-semibold tracking-[-0.01em]">{group.title}</h2>
            <ul className="grid gap-3">
              {group.pages.map((slug) => (
                <li key={slug}>
                  <a href={href(`/${slug}`)} className="group block">
                    <span className="text-[14.5px] font-medium text-foreground group-hover:text-primary">{pages[slug].title}</span>
                    <span className="block text-[13px] leading-snug text-muted">{pages[slug].description}</span>
                  </a>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </div>
  );
}
