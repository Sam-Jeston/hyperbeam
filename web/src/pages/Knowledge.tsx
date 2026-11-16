// The knowledge base: the documents agents and people have written for the
// team, or, when the team keeps them in Notion, where they go.

import { Bot, ExternalLink, FileText, Pencil, Plus, Search, Settings2, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Button } from "../components/ui/button";
import { RadioCards } from "../components/ui/controls";
import { confirm, Modal } from "../components/ui/dialog";
import { Field, Input, Textarea } from "../components/ui/input";
import { Badge, Card, EmptyState, Notice, Skeleton } from "../components/ui/surface";
import { api, type DocumentSummary, type KnowledgeBase, type KnowledgeDocument } from "../lib/api";
import { useStore } from "../lib/store";
import { A, Avatar, ErrorNote, Ext, isSubmitKey, Markdown, navigate, openComposer, Page, PageHeader, Time, useAction, useDocumentTitle, useLoad } from "../lib/ui";
import { ServiceLogo } from "./Connections";

export function KnowledgePage({ id }: { id?: string }) {
  return id ? <DocumentPage key={id} id={id} /> : <Home />;
}

function Home() {
  useDocumentTitle("Knowledge base");
  const isAdmin = useStore((s) => s.me?.role !== "member");
  const kb = useLoad<KnowledgeBase>(() => api.get("/knowledge-base"), []);
  const [configuring, setConfiguring] = useState(false);
  const [writing, setWriting] = useState(false);
  const inNotion = kb.data?.config.mode === "notion";
  return (
    <Page>
      <PageHeader
        title="Knowledge base"
        description="What your agents have found out: reports, investigations and designs, published where the whole team can find them. Agents search it before they repeat the work."
        actions={
          <>
            {isAdmin && kb.data && <Button onClick={() => setConfiguring(true)}><Settings2 /> Where documents go</Button>}
            {kb.data && !inNotion && <Button variant="primary" onClick={() => setWriting(true)}><Plus /> New document</Button>}
          </>
        }
      />
      <ErrorNote error={kb.error} />
      {!kb.data && !kb.error && <ListSkeleton />}
      {kb.data && inNotion && <NotionHome kb={kb.data} />}
      {kb.data && !inNotion && <Documents empty={kb.data.documents === 0} />}
      {configuring && kb.data && <Configure kb={kb.data} onClose={() => (setConfiguring(false), kb.reload())} />}
      {writing && <DocumentEditor document={null} onClose={(saved) => (setWriting(false), saved && navigate(`/knowledge/${saved.id}`))} />}
    </Page>
  );
}

function ListSkeleton() {
  return <Card className="divide-y divide-border">{[0, 1, 2].map((i) => <div key={i} className="grid gap-2 px-4 py-4"><Skeleton className="h-4 w-64" /><Skeleton className="h-4 w-full max-w-[520px]" /></div>)}</Card>;
}

/** With documents kept in Notion there is nothing to browse here, only where they go. */
function NotionHome({ kb }: { kb: KnowledgeBase }) {
  const target = kb.config.notion;
  return (
    <div className="grid gap-8">
      <Card className="flex flex-wrap items-center gap-4 p-5">
        <ServiceLogo id="notion" title="Notion" />
        <div className="grid min-w-0 flex-[1_1_300px] gap-0.5">
          <div className="font-display text-[15.5px] font-semibold">Documents are kept in Notion</div>
          <p className="max-w-[70ch] text-[13.5px] text-muted">
            Each document an agent publishes becomes a page under{" "}
            {target ? <Ext href={target.url}>{target.title || "the page your team chose"}</Ext> : "a page your team chose"}. Read and search them there. The session and
            workstream that produced a document link to its page.
          </p>
        </div>
        {target && <Button variant="primary" onClick={() => window.open(target.url, "_blank", "noopener")}><ExternalLink /> Open in Notion</Button>}
      </Card>
      {!kb.notion_connected && (
        <Notice tone="warning" title="Notion is not connected">
          Agents cannot publish documents until an admin reconnects Notion under <A href="/settings/integrations" className="text-primary hover:underline">Settings → Integrations</A>, or moves the knowledge base back into Hyperbeam.
        </Notice>
      )}
      {kb.documents > 0 && (
        <section>
          <h2 className="font-display text-[15.5px] font-semibold tracking-[-0.01em]">Still in Hyperbeam</h2>
          <p className="mb-3 mt-0.5 text-[13px] text-muted">Documents published before the knowledge base moved to Notion. They stay readable here.</p>
          <Documents empty={false} />
        </section>
      )}
    </div>
  );
}

function Documents({ empty }: { empty: boolean }) {
  const [typed, setTyped] = useState("");
  const [query, setQuery] = useState("");
  // Search as people type, without a request per keystroke.
  useEffect(() => {
    const timer = setTimeout(() => setQuery(typed.trim()), 200);
    return () => clearTimeout(timer);
  }, [typed]);
  const { data, error } = useLoad<{ documents: DocumentSummary[] }>(() => api.get(`/documents?q=${encodeURIComponent(query)}`), [query]);

  if (empty) {
    return (
      <EmptyState icon={<FileText />} title="No documents yet" action={<Button variant="primary" onClick={() => openComposer()}><Plus /> New work</Button>}>
        Ask an agent to investigate something, review a design or write up what it found. Its report is published here instead of being left in its sandbox.
      </EmptyState>
    );
  }
  return (
    <>
      <div className="relative mb-4 max-w-[440px]">
        <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-faint" />
        <Input type="search" aria-label="Search the knowledge base" placeholder="Search documents" className="pl-9" value={typed} onChange={(e) => setTyped(e.target.value)} />
      </div>
      <ErrorNote error={error} />
      {!data && !error && <ListSkeleton />}
      {data && data.documents.length === 0 && <p className="py-6 text-[13.5px] text-muted">No documents match “{query}”.</p>}
      {data && data.documents.length > 0 && (
        <Card className="divide-y divide-border overflow-hidden">{data.documents.map((doc) => <DocumentRow key={doc.id} doc={doc} />)}</Card>
      )}
    </>
  );
}

function DocumentRow({ doc }: { doc: DocumentSummary }) {
  const author = useStore((s) => (doc.created_by ? s.users[doc.created_by] : undefined));
  const workstream = useStore((s) => (doc.workstream_id ? s.workstreams[doc.workstream_id] : undefined));
  return (
    <A href={`/knowledge/${doc.id}`} className="grid gap-1 px-4 py-3.5 transition-colors hover:bg-surface-2/60 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring">
      <span className="font-medium text-foreground [overflow-wrap:anywhere]">{doc.title}</span>
      {doc.summary && <span className="line-clamp-2 max-w-[90ch] text-[13px] text-muted">{doc.summary}</span>}
      <span className="mt-0.5 flex flex-wrap items-center gap-x-2.5 gap-y-1 text-[12px] text-faint">
        {doc.session_id && <Badge><Bot /> Agent</Badge>}
        {author && <span className="flex items-center gap-1.5"><Avatar user={author} size={16} className="ring-0" />{doc.session_id ? `for ${author.name}` : author.name}</span>}
        {workstream && <span className="truncate">· {workstream.title}</span>}
        <span>·</span>
        <Time iso={doc.updated_at} />
      </span>
    </A>
  );
}

function DocumentPage({ id }: { id: string }) {
  const { data, error, reload } = useLoad<{ document: KnowledgeDocument }>(() => api.get(`/documents/${id}`), [id]);
  const doc = data?.document;
  useDocumentTitle(doc?.title ?? "Knowledge base");
  const users = useStore((s) => s.users);
  const session = useStore((s) => (doc?.session_id ? s.sessions[doc.session_id] : undefined));
  const workstream = useStore((s) => (doc?.workstream_id ? s.workstreams[doc.workstream_id] : undefined));
  const [editing, setEditing] = useState(false);
  const action = useAction();
  const crumbs = [{ label: "Knowledge base", href: "/knowledge" }];

  if (error) {
    return (
      <Page width="narrow">
        <PageHeader title="Document not found" breadcrumbs={crumbs} description="It may have been deleted, or the link is out of date." />
        <Button onClick={() => navigate("/knowledge")}>Back to the knowledge base</Button>
      </Page>
    );
  }
  if (!doc) return <Page width="narrow"><Skeleton className="mb-6 h-8 w-80" /><Skeleton className="h-96 rounded-lg" /></Page>;

  const author = doc.created_by ? users[doc.created_by] : undefined;
  const editor = doc.updated_by ? users[doc.updated_by] : undefined;
  const remove = async () => {
    if (!(await confirm({ title: `Delete “${doc.title}”?`, description: "The document is removed from the knowledge base, and from the session and workstream that link to it. This cannot be undone.", confirmLabel: "Delete", destructive: true }))) return;
    if (await action.run(() => api.del(`/documents/${doc.id}`), { success: "Document deleted", toastError: true })) navigate("/knowledge");
  };
  // A document that opens with its own title as a heading would show it twice.
  const body = doc.content.replace(/^\s*#\s+(.*)\n+/, (heading, text: string) => (text.trim().toLowerCase() === doc.title.trim().toLowerCase() ? "" : heading));
  return (
    <Page width="narrow">
      <PageHeader
        title={doc.title}
        breadcrumbs={crumbs}
        description={doc.summary || undefined}
        actions={
          <>
            <Button onClick={() => setEditing(true)}><Pencil /> Edit</Button>
            <Button variant="danger-ghost" disabled={action.busy} onClick={() => void remove()}><Trash2 /> Delete</Button>
          </>
        }
        meta={
          <>
            {doc.session_id ? (
              <span className="flex min-w-0 max-w-full items-center gap-1.5">
                <Badge><Bot /> Agent</Badge>
                {session ? <A href={`/s/${doc.session_id}`} title={session.title} className="min-w-0 max-w-[36ch] truncate text-primary hover:underline">{session.title}</A> : "a session"}
                {author && <span className="shrink-0">for {author.name}</span>}
              </span>
            ) : (
              author && <span className="flex items-center gap-1.5"><Avatar user={author} size={18} className="ring-0" />{author.name}</span>
            )}
            {workstream && <span>· in <A href={`/w/${workstream.id}`} className="text-primary hover:underline">{workstream.title}</A></span>}
            <span>· published <Time iso={doc.created_at} /></span>
            {editor && <span>· edited by {editor.name} <Time iso={doc.updated_at} /></span>}
          </>
        }
      />
      <Card className="px-5 py-5 sm:px-8 sm:py-7">
        <Markdown text={body} className="prose-doc" />
      </Card>
      {editing && <DocumentEditor document={doc} onClose={() => (setEditing(false), reload())} />}
    </Page>
  );
}

function DocumentEditor({ document, onClose }: { document: KnowledgeDocument | null; onClose: (saved?: KnowledgeDocument) => void }) {
  const [title, setTitle] = useState(document?.title ?? "");
  const [summary, setSummary] = useState(document?.summary ?? "");
  const [content, setContent] = useState(document?.content ?? "");
  const action = useAction();
  const valid = !!title.trim() && !!content.trim();
  const save = () =>
    valid &&
    action.run(async () => {
      const body = { title, summary, content };
      const saved = await (document ? api.put(`/documents/${document.id}`, body) : api.post("/documents", body));
      onClose(saved.document);
    }, { success: document ? "Document saved" : "Document published" });
  return (
    <Modal
      title={document ? "Edit document" : "New document"}
      onClose={() => onClose()}
      size="lg"
      modalOnly
      footer={
        <>
          <Button onClick={() => onClose()}>Cancel</Button>
          <Button variant="primary" disabled={!valid} loading={action.busy} onClick={() => void save()}>{document ? "Save document" : "Publish document"}</Button>
        </>
      }
    >
      <Field label="Title">
        {(id) => <Input id={id} value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Checkout latency investigation" />}
      </Field>
      <Field label="Summary" optional hint="One sentence saying what it found or decided. Shown in lists and search results; left blank, the first line of the document is used.">
        {(id) => <Input id={id} value={summary} onChange={(e) => setSummary(e.target.value)} />}
      </Field>
      <Field label="Document" hint="Markdown.">
        {(id) => <Textarea id={id} mono rows={18} value={content} onChange={(e) => setContent(e.target.value)} onKeyDown={(e) => isSubmitKey(e) && void save()} />}
      </Field>
      <ErrorNote error={action.error} />
    </Modal>
  );
}

function Configure({ kb, onClose }: { kb: KnowledgeBase; onClose: () => void }) {
  const [mode, setMode] = useState(kb.config.mode);
  const [page, setPage] = useState(kb.config.notion?.url ?? "");
  const action = useAction();
  const blocked = mode === "notion" && (!kb.notion_connected || !page.trim());
  const save = () =>
    action.run(async () => {
      const saved = await api.put<KnowledgeBase>("/knowledge-base", { mode, notion_page: mode === "notion" ? page : undefined });
      if (saved.caution) toast.warning(saved.caution, { duration: 12000 });
      else toast.success(mode === "notion" ? "Documents now go to Notion" : "Documents are now kept in Hyperbeam");
      onClose();
    });
  return (
    <Modal
      title="Where documents go"
      description="When an agent writes a report, an investigation or a design, it publishes it to the team's knowledge base. Choose where that is."
      onClose={onClose}
      modalOnly
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={blocked} loading={action.busy} onClick={() => void save()}>Save</Button>
        </>
      }
    >
      <RadioCards
        aria-label="Where documents go"
        value={mode}
        onValueChange={setMode}
        options={[
          { value: "internal", title: "In Hyperbeam", description: "Kept here. The team reads, searches and edits documents on this page, and agents search them before starting work." },
          { value: "notion", title: "In Notion", description: "Each document becomes a page under one you choose. The team reads and searches them in Notion." },
        ]}
      />
      {mode === "notion" && !kb.notion_connected && (
        <Notice tone="warning" title="Connect Notion first">
          Hyperbeam creates the pages through its Notion connection. Add it under <A href="/settings/integrations" className="text-primary hover:underline">Settings → Integrations</A>, then come back.
        </Notice>
      )}
      {mode === "notion" && kb.notion_connected && (
        <>
          <Field label="Notion page" hint="The page documents are created under. In Notion, open it and use Share → Copy link. The account Hyperbeam is signed in to Notion with must be able to add pages to it.">
            {(id) => <Input id={id} value={page} onChange={(e) => setPage(e.target.value)} placeholder="https://www.notion.so/acme/Agent-reports-1f0c2b3a…" onKeyDown={(e) => e.key === "Enter" && !blocked && void save()} />}
          </Field>
          <p className="text-[12.5px] leading-snug text-muted">
            Agents publish there whatever Notion permissions their session holds: this setting is what allows it, and it reaches only that page. To find earlier documents, an agent needs permission to read Notion.
            {kb.documents > 0 && ` The ${kb.documents === 1 ? "document" : `${kb.documents} documents`} already in Hyperbeam ${kb.documents === 1 ? "stays" : "stay"} here.`}
          </p>
        </>
      )}
      <ErrorNote error={action.error} />
    </Modal>
  );
}
