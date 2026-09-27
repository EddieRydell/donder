import { BaseEdge, getBezierPath, type EdgeProps } from "@xyflow/react";
import { useLayoutEffect, useRef } from "react";
import { THEME_METRICS } from "../../../theme";

export function GraphFlowEdge({ id, sourceX, sourceY, sourcePosition, targetX, targetY, targetPosition,
  markerEnd, style = {}, interactionWidth = THEME_METRICS.graphEdgeInteractionWidth }: EdgeProps) {
  const [path] = getBezierPath({ sourceX, sourceY, sourcePosition, targetX, targetY, targetPosition });
  const group = useRef<SVGGElement>(null);
  const direction = useRef<SVGPathElement>(null);
  useLayoutEffect(() => {
    const connector = group.current?.querySelector<SVGPathElement>(".react-flow__edge-path");
    if (connector === undefined || connector === null || direction.current === null) return;
    const middle = connector.getTotalLength() / 2;
    const before = connector.getPointAtLength(Math.max(0, middle - THEME_METRICS.graphArrowTangentSample));
    const center = connector.getPointAtLength(middle);
    // Measure the rendered curve so the arrow follows its tangent at half its arc length.
    direction.current.setAttribute("d", `M ${before.x} ${before.y} L ${center.x} ${center.y}`);
  }, [path]);
  return <g ref={group}>
    <BaseEdge id={id} path={path} style={style} interactionWidth={interactionWidth} />
    <path ref={direction} className="graph-flow-direction" markerEnd={markerEnd} aria-hidden="true" />
  </g>;
}
