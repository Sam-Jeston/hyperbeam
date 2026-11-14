import { Boxes, CircleCheck, Download, RefreshCw, Server, TriangleAlert } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { Button } from "../components/ui/button";
import { Switch } from "../components/ui/controls";
import { Tooltip } from "../components/ui/menu";
import { Badge, Card, EmptyState, Notice, Skeleton } from "../components/ui/surface";
import { api, type Cluster, type ClusterImage, type ClusterNode, type ClusterPull, type ClusterSandbox } from "../lib/api";
import { A, ErrorNote, Page, PageHeader, SectionTitle, Time, useDocumentTitle, useLoad } from "../lib/ui";
import { cn } from "../lib/utils";

/** How often the page asks again while it is in view. */
const REFRESH_MS = 5000;

function bytes(n: number | null | undefined): string {
  if (n == null) return "";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = n;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${value >= 100 || unit === 0 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}

const nodeHealthy = (node: ClusterNode) => node.ready && node.problems.length === 0;

/** Where an image stands across the nodes that could run a sandbox. */
function imageStanding(image: ClusterImage): "failing" | "present" | "partial" | "missing" | "unknown" {
  if (image.pull_error) return "failing";
  if (image.nodes.length === 0) return "unknown";
  const present = image.nodes.filter((n) => n.state === "present").length;
  if (present === image.nodes.length) return "present";
  if (image.nodes.some((n) => n.state === "unknown")) return present > 0 ? "partial" : "unknown";
  return present > 0 ? "partial" : "missing";
}

export function ClusterPage() {
  useDocumentTitle("Cluster");
  const { data, error, reload } = useLoad<Cluster>(() => api.get("/cluster"), []);
  useEffect(() => {
    const id = setInterval(() => document.visibilityState === "visible" && reload(), REFRESH_MS);
    return () => clearInterval(id);
  }, []);

  return (
    <Page>
      <PageHeader
        title="Cluster"
        description="Where agents run: the nodes, the images their sandboxes start from, and anything that is failing."
        actions={
          <>
            {data && <span className="text-[12.5px] text-faint">Checked <Time iso={data.checked_at} /></span>}
            <Button onClick={reload}><RefreshCw /> Refresh</Button>
          </>
        }
      />
      <ErrorNote error={error} title={data ? "Could not refresh. This may be out of date." : undefined} />
      {!data && !error && (
        <div className="grid gap-3">
          <Skeleton className="h-20 rounded-lg" />
          <Skeleton className="h-40 rounded-lg" />
          <Skeleton className="h-40 rounded-lg" />
        </div>
      )}
      {data && <ClusterBody cluster={data} reload={reload} />}
    </Page>
  );
}

/** Asking nodes to fetch images afresh, and what went wrong asking. */
function usePulls(reload: () => void) {
  const [asking, setAsking] = useState<Set<string>>(new Set());
  const [errors, setErrors] = useState<Record<string, string>>({});
  const pull = async (images: string[]) => {
    setAsking((a) => new Set([...a, ...images]));
    for (const image of images) {
      try {
        await api.post("/cluster/images/pull", { image });
        setErrors(({ [image]: _, ...rest }) => rest);
      } catch (e) {
        setErrors((r) => ({ ...r, [image]: e instanceof Error ? e.message : String(e) }));
      }
    }
    setAsking((a) => new Set([...a].filter((i) => !images.includes(i))));
    reload();
  };
  return { asking, errors, pull };
}

const pulling = (image: ClusterImage) => image.pulls.some((p) => p.state === "pulling");

function ClusterBody({ cluster, reload }: { cluster: Cluster; reload: () => void }) {
  const kubernetes = cluster.driver === "kubernetes";
  const pulls = usePulls(reload);
  const idle = cluster.images.filter((i) => !pulling(i) && !pulls.asking.has(i.image));
  const badNodes = cluster.nodes.filter((n) => !nodeHealthy(n));
  const failing = cluster.sandboxes.filter((s) => s.state === "failing");
  const badImages = cluster.images.filter((i) => i.pull_error);
  const pulled = cluster.images.filter((i) => imageStanding(i) === "present").length;
  const problems = badNodes.length + failing.length + badImages.length;
  const unreadable = [cluster.nodes_error, cluster.sandboxes_error, cluster.events_error].filter(Boolean).length;

  return (
    <div className="mt-4 grid gap-8">
      <div className="grid gap-3">
        <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
          <Stat
            label="Status"
            tone={problems ? "fail" : unreadable ? "wait" : "run"}
            value={problems ? `${problems} problem${problems === 1 ? "" : "s"}` : unreadable ? "Partly unknown" : "Healthy"}
            icon={problems || unreadable ? <TriangleAlert /> : <CircleCheck />}
          />
          <Stat label="Agents live" value={`${cluster.live_sessions} of ${cluster.max_sessions}`} hint="Sessions with a sandbox, against the most allowed at once." />
          {kubernetes ? (
            <>
              <Stat
                label="Nodes ready"
                tone={badNodes.length ? "fail" : undefined}
                value={cluster.nodes_error ? "Unknown" : `${cluster.nodes.filter((n) => n.ready).length} of ${cluster.nodes.length}`}
              />
              <Stat
                label="Images pulled"
                tone={badImages.length ? "fail" : undefined}
                value={cluster.nodes_error ? "Unknown" : `${pulled} of ${cluster.images.length}`}
                hint="Images held by every node, so a sandbox starts without waiting for a download."
              />
            </>
          ) : (
            <Stat label="Sandboxes" value={String(cluster.sandboxes.length)} />
          )}
        </div>
        {!kubernetes && (
          <Notice title="Agents run as local processes">
            This server uses the process driver: each agent is a process on the server’s own machine, with no isolation, no nodes and no images. Install Hyperbeam on Kubernetes to run each agent in its own pod.
          </Notice>
        )}
      </div>

      {kubernetes && (
        <section>
          <SectionTitle description="The machines sandboxes are scheduled onto.">Nodes</SectionTitle>
          <ErrorNote error={cluster.nodes_error} title="Nodes can’t be read" />
          {cluster.nodes.length > 0 && (
            <Card className="divide-y divide-border">
              {cluster.nodes.map((node) => <NodeRow key={node.name} node={node} />)}
            </Card>
          )}
        </section>
      )}

      {kubernetes && (
        <section>
          <SectionTitle
            description={imagesBlurb(cluster)}
            action={
              cluster.images.length > 1 && (
                <Tooltip content="Fetch every image again from its registry onto each ready node.">
                  <Button size="sm" disabled={idle.length === 0} onClick={() => pulls.pull(idle.map((i) => i.image))}>
                    <Download /> Pull all
                  </Button>
                </Tooltip>
              )
            }
          >
            Images
          </SectionTitle>
          <Card className="divide-y divide-border">
            {cluster.images.map((image) => (
              <ImageRow
                key={image.image}
                image={image}
                cluster={cluster}
                asking={pulls.asking.has(image.image)}
                error={pulls.errors[image.image]}
                onPull={() => pulls.pull([image.image])}
              />
            ))}
          </Card>
        </section>
      )}

      <section>
        <SectionTitle description={kubernetes ? `One pod per live agent, in the ${cluster.namespace} namespace.` : "One runner process per live agent."}>Sandboxes</SectionTitle>
        <ErrorNote error={cluster.sandboxes_error} title="Sandboxes can’t be read" />
        {cluster.sandboxes.length > 0 ? (
          <Card className="divide-y divide-border">
            {cluster.sandboxes.map((sandbox) => <SandboxRow key={sandbox.name + sandbox.id} sandbox={sandbox} />)}
          </Card>
        ) : !cluster.sandboxes_error && (
          <EmptyState compact icon={<Boxes />} title="No sandboxes right now">A sandbox exists only while its agent is working or waiting for a reply.</EmptyState>
        )}
      </section>

      {kubernetes && <Events cluster={cluster} />}
    </div>
  );
}

function imagesBlurb(cluster: Cluster): string {
  const policy = cluster.sandbox?.pull_policy;
  const base = "What sandboxes start from: the runner, each harness, and each environment.";
  if (policy === "Always") return `${base} The pull policy is Always, so each sandbox checks the registry when it starts.`;
  if (policy === "Never") return `${base} The pull policy is Never, so an image a node lacks cannot be used there.`;
  return `${base} An image a node has not pulled is downloaded when the first sandbox needs it. Pull one to fetch what its tag names now, such as a newer latest.`;
}

function Stat(props: { label: string; value: string; tone?: "run" | "wait" | "fail"; icon?: ReactNode; hint?: string }) {
  const body = (
    <Card className="grid gap-1 px-4 py-3">
      <div className="text-[12px] font-medium text-muted">{props.label}</div>
      <div
        className={cn(
          "flex items-center gap-1.5 font-display text-[19px] font-semibold tracking-[-0.01em] [&_svg]:size-4.5",
          props.tone === "fail" ? "text-fail" : props.tone === "wait" ? "text-wait" : props.tone === "run" ? "text-run" : undefined,
        )}
      >
        {props.icon}
        {props.value}
      </div>
    </Card>
  );
  return props.hint ? <Tooltip content={props.hint}>{body}</Tooltip> : body;
}

function Dot({ tone }: { tone: "run" | "wait" | "fail" | "neutral" }) {
  return <span className={cn("mt-1.5 size-2 shrink-0 rounded-full", tone === "run" ? "bg-run" : tone === "wait" ? "bg-wait" : tone === "fail" ? "bg-fail" : "bg-faint")} />;
}

function NodeRow({ node }: { node: ClusterNode }) {
  const facts = [node.arch, node.cpu && `${node.cpu} CPU`, bytes(node.memory_bytes) && `${bytes(node.memory_bytes)} memory`, node.os, node.kubelet, node.runtime].filter(Boolean);
  return (
    <div className="flex items-start gap-3 px-4 py-3">
      <Dot tone={nodeHealthy(node) ? "run" : "fail"} />
      <div className="grid min-w-0 flex-1 gap-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="font-mono text-[13px] font-medium">{node.name}</span>
          {node.ready ? <Badge tone="run">Ready</Badge> : <Badge tone="fail">Not ready</Badge>}
          {!node.schedulable && (
            <Tooltip content="The node is cordoned: no new sandboxes are scheduled onto it.">
              <Badge tone="wait">Cordoned</Badge>
            </Tooltip>
          )}
          {node.roles.map((role) => <Badge key={role} tone="outline">{role}</Badge>)}
        </div>
        <div className="text-[12.5px] text-muted">{facts.join(" · ")}</div>
        {node.problems.map((problem) => <div key={problem} className="text-[12.5px] text-fail [overflow-wrap:anywhere]">{problem}</div>)}
      </div>
    </div>
  );
}

const USE_LABEL = { runner: "Runner", harness: "Harness", environment: "Environment" };

function PullLine({ pull, several }: { pull: ClusterPull; several: boolean }) {
  const on = several ? ` on ${pull.node}` : "";
  if (pull.state === "pulling") return <div className="text-[12.5px] text-wait">Pulling{on}…</div>;
  if (pull.state === "failed") return <div className="text-[12.5px] text-fail [overflow-wrap:anywhere]">Pull failed{on}: {pull.message}</div>;
  return (
    <div className="text-[12.5px] text-run">
      Pulled afresh{on} <Time iso={pull.finished_at ?? pull.started_at} />
    </div>
  );
}

function ImageRow(props: { image: ClusterImage; cluster: Cluster; asking: boolean; error?: string; onPull: () => void }) {
  const { image, cluster } = props;
  const standing = imageStanding(image);
  const present = image.nodes.filter((n) => n.state === "present");
  const several = image.nodes.length > 1;
  const size = present.find((n) => n.size_bytes)?.size_bytes;
  const never = cluster.sandbox?.pull_policy === "Never";
  const where = several ? (
    <div className="grid gap-0.5">
      {image.nodes.map((n) => <div key={n.node}>{n.node}: {n.state === "present" ? "pulled" : n.state === "missing" ? "not pulled" : "unknown"}</div>)}
    </div>
  ) : null;
  const badge =
    standing === "failing" ? <Badge tone="fail">Pull failing</Badge> :
    standing === "present" ? <Badge tone="run">{several ? `Pulled on all ${image.nodes.length} nodes` : "Pulled"}</Badge> :
    standing === "partial" ? <Badge tone="wait">Pulled on {present.length} of {image.nodes.length} nodes</Badge> :
    standing === "missing" ? <Badge tone={never ? "fail" : "wait"}>Not pulled</Badge> :
    <Badge>Unknown</Badge>;
  const unknownWhy = cluster.nodes_error
    ? "Hyperbeam can’t read the cluster’s nodes, so it can’t tell which images they hold."
    : "The node does not list this image, but it reports only its 50 largest, so it may still hold it.";
  return (
    <div className="flex items-start gap-3 px-4 py-3">
      <Dot tone={standing === "failing" ? "fail" : standing === "present" ? "run" : standing === "unknown" ? "neutral" : never && standing === "missing" ? "fail" : "wait"} />
      <div className="grid min-w-0 flex-1 gap-1">
        <div className="font-mono text-[12.5px] [overflow-wrap:anywhere]">{image.image}</div>
        <div className="flex flex-wrap gap-1.5">
          {image.used_by.map((use) => (
            <Badge key={use.kind + use.name} tone="outline">{use.kind === "runner" ? USE_LABEL.runner : `${USE_LABEL[use.kind]} · ${use.name}`}</Badge>
          ))}
        </div>
        {image.pull_error && <div className="text-[12.5px] text-fail [overflow-wrap:anywhere]">{image.pull_error}</div>}
        {image.pulls.map((pull) => <PullLine key={pull.node} pull={pull} several={several} />)}
        {props.error && <div className="text-[12.5px] text-fail [overflow-wrap:anywhere]">Could not start a pull: {props.error}</div>}
      </div>
      <div className="flex shrink-0 items-center gap-2">
        {size ? <span className="hidden text-[12px] tabular-nums text-faint sm:inline">{bytes(size)}</span> : null}
        {where ? <Tooltip content={where}>{badge}</Tooltip> : standing === "unknown" ? <Tooltip content={unknownWhy}>{badge}</Tooltip> : badge}
        <Tooltip content="Fetch this image again from its registry onto each ready node, so new sandboxes start from what its tag names now.">
          <Button size="icon-sm" variant="ghost" aria-label={`Pull ${image.image}`} loading={props.asking || pulling(image)} onClick={props.onPull}>
            {!(props.asking || pulling(image)) && <Download />}
          </Button>
        </Tooltip>
      </div>
    </div>
  );
}

const SANDBOX_STATE: Record<ClusterSandbox["state"], { label: string; tone: "run" | "wait" | "fail" | "neutral" }> = {
  running: { label: "Running", tone: "run" },
  starting: { label: "Starting", tone: "wait" },
  failing: { label: "Failing", tone: "fail" },
  finished: { label: "Finished", tone: "neutral" },
  terminating: { label: "Shutting down", tone: "neutral" },
};

function SandboxRow({ sandbox }: { sandbox: ClusterSandbox }) {
  const { label, tone } = SANDBOX_STATE[sandbox.state];
  const owner = sandbox.owner;
  const detail = [sandbox.reason, sandbox.message].filter(Boolean).join(": ");
  return (
    <div className="flex items-start gap-3 px-4 py-3">
      <Dot tone={tone} />
      <div className="grid min-w-0 flex-1 gap-1">
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          {owner.kind === "session" ? (
            <A href={`/s/${owner.session_id}`} className="truncate text-[13.5px] font-medium hover:text-primary">{owner.title}</A>
          ) : owner.kind === "probe" ? (
            <span className="truncate text-[13.5px] font-medium">Model discovery{owner.title ? ` for ${owner.title}` : ""}</span>
          ) : (
            <Tooltip content="No live session owns this sandbox. Hyperbeam removes such sandboxes when it starts.">
              <span className="text-[13.5px] font-medium text-muted">No session</span>
            </Tooltip>
          )}
          <Badge tone={tone}>{label}</Badge>
          {sandbox.restarts > 0 && <Badge tone="wait">{sandbox.restarts} restart{sandbox.restarts === 1 ? "" : "s"}</Badge>}
        </div>
        <div className="text-[12.5px] text-muted [overflow-wrap:anywhere]">
          <span className="font-mono text-[12px]">{sandbox.name}</span>
          {sandbox.node && <> · on {sandbox.node}</>}
          {sandbox.image && <> · <span className="font-mono text-[12px]">{sandbox.image}</span></>}
        </div>
        {detail && <div className={cn("text-[12.5px] [overflow-wrap:anywhere]", sandbox.state === "failing" ? "text-fail" : "text-muted")}>{detail}</div>}
      </div>
      <Time iso={sandbox.created_at} className="text-[12px] text-faint" />
    </div>
  );
}

function Events({ cluster }: { cluster: Cluster }) {
  const [warningsOnly, setWarningsOnly] = useState(true);
  const events = cluster.events.filter((e) => !warningsOnly || e.warning);
  return (
    <section>
      <SectionTitle
        description="What Kubernetes has reported about sandboxes lately: scheduling, image pulls, crashes. It keeps events for about an hour."
        action={
          <label className="flex shrink-0 cursor-pointer items-center gap-2 text-[13px] font-medium">
            <Switch checked={warningsOnly} onCheckedChange={setWarningsOnly} aria-label="Warnings only" /> Warnings only
          </label>
        }
      >
        Events
      </SectionTitle>
      <ErrorNote error={cluster.events_error} title="Events can’t be read" />
      {events.length > 0 ? (
        <Card className="overflow-x-auto">
          <table className="w-full min-w-[720px] border-collapse text-[13px]">
            <thead>
              <tr className="border-b border-border text-left text-[12px] text-muted">
                {["When", "Reason", "About", "Message"].map((h) => <th key={h} className="px-4 py-2.5 font-medium">{h}</th>)}
              </tr>
            </thead>
            <tbody className="divide-y divide-border">
              {events.map((event, i) => (
                <tr key={i} className="align-top hover:bg-surface-2/40">
                  <td className="px-4 py-2 text-muted"><Time iso={event.ts} /></td>
                  <td className="px-4 py-2">
                    <Badge tone={event.warning ? "fail" : "neutral"}>{event.reason}{event.count > 1 ? ` ×${event.count}` : ""}</Badge>
                  </td>
                  <td className="max-w-56 truncate px-4 py-2 font-mono text-[12px]" title={event.object}>{event.object}</td>
                  <td className="px-4 py-2 text-muted [overflow-wrap:anywhere]">{event.message}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      ) : !cluster.events_error && (
        <EmptyState compact icon={<Server />} title={warningsOnly ? "No warnings" : "No events"}>
          {warningsOnly ? "Kubernetes has reported nothing wrong with sandboxes in the last hour." : "Nothing has happened to sandboxes in the last hour."}
        </EmptyState>
      )}
    </section>
  );
}
