import { MARK_FILLS, MARK_LAYERS } from "../../../../brand/mark";
import { cn } from "../../lib/utils";

/** The Hyperbeam mark: a pixel beam fired corner to corner from a charged orb. */
export function BrandMark({ size = 24, className }: { size?: number; className?: string }) {
  return (
    <svg viewBox="0 0 16 16" width={size} height={size} shapeRendering="crispEdges" className={cn("shrink-0", className)} aria-hidden>
      {MARK_FILLS.map(([layer, fill]) => <path key={layer} d={MARK_LAYERS[layer]} fill={fill} />)}
    </svg>
  );
}

export function Wordmark({ className }: { className?: string }) {
  return <span className={cn("font-display font-bold tracking-[-0.035em]", className)}>hyperbeam</span>;
}
