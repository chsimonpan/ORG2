// SVG marker shapes adapted from MIT context-visualizer bfd2b50d, Zaheer2026.
// React canonical adapter, not the Node JSONL model or global innerHTML renderer.
import {useId,useMemo,useState} from 'react';
import type {CanonicalNode,JourneySnapshot,MarkerKind,DurableAnchor} from './types';
import {buildTimeline,ribbonPath,sample,starPath} from './visualAlgorithms';
const colors=['#17a2a6','#e7ad38','#c45b80','#657edf','#5da76b'];
function Marker({kind,color}:{kind:MarkerKind;color:string}) {
 const dark='var(--bg,#18222c)';
 switch(kind){
 case 'decision':return <path d="M0,-7 L7,0 L0,7 L-7,0 Z" fill={color} stroke={dark} strokeWidth={2}/>;
 case 'pivot':return <><circle r={11} fill={color} opacity={.14}/><path d={starPath(8,3.4,6)} fill={color} stroke={dark} strokeWidth={1.6}/></>;
 case 'abandon':return <><circle r={8} fill={dark} stroke={color}/><path d="M-3.4,-3.4 L3.4,3.4 M3.4,-3.4 L-3.4,3.4" stroke={color} strokeWidth={2}/></>;
 case 'artifact':return <><rect x={-6.5} y={-6.5} width={13} height={13} rx={3} fill={color}/><path d="M-3,-2 H3 M-3,1 H3 M-3,4 H1" stroke={dark}/></>;
 case 'finding':return <><circle r={6.5} fill={color}/><circle r={2.4} fill={dark}/></>;
 case 'question':return <><circle r={8} fill={dark} stroke={color}/><text textAnchor="middle" y={3.5} fontSize={10} fill={color}>?</text></>;
 case 'verify':return <><circle r={8} fill={color}/><path d="M-3.4,0 L-1,2.6 L3.6,-2.8" fill="none" stroke={dark} strokeWidth={2.1}/></>;
 case 'handoff':return <><circle r={8} fill={dark} stroke={color}/><path d="M-3.5,0 H3 M0.5,-2.6 L3.6,0 L0.5,2.6" fill="none" stroke={color} strokeWidth={1.8}/></>;
 case 'interrupt':return <><path d="M0,-7.5 L7,6 L-7,6 Z" fill={color}/><text textAnchor="middle" y={5} fontSize={9} fill={dark}>!</text></>;
 }
}
export function JourneyCanvas(props:{snapshot:JourneySnapshot;onSelect:(anchor:DurableAnchor)=>void}) {
 return <JourneyCanvasOwner key={props.snapshot.ownerSessionId} {...props}/>;
}
function JourneyCanvasOwner({snapshot,onSelect}:{snapshot:JourneySnapshot;onSelect:(anchor:DurableAnchor)=>void}) {
 const uid=useId(),[scale,setScale]=useState(1),[pan,setPan]=useState({x:0,y:0}),[selected,setSelected]=useState<string|null>(null),[collapsed,setCollapsed]=useState<Set<string>>(new Set());
 const timeline=useMemo(()=>buildTimeline(snapshot.nodes.map(n=>n.timestamp)),[snapshot]);
 const lanes=[...new Set(snapshot.nodes.map(n=>n.laneId))].sort();
 const pick=(n:CanonicalNode)=>{if(n.anchor.ownerSessionId!==snapshot.ownerSessionId)return;setSelected(n.id);onSelect(n.anchor);};
 const kinds:MarkerKind[]=['question','decision','pivot','abandon','artifact','finding','verify','handoff','interrupt'];
 if(!snapshot.capabilities.read)return <p>需授权加载 Journey</p>;
 return <section aria-label="Canonical Journey timeline" data-owner={snapshot.ownerSessionId}>
 <div role="toolbar" aria-label="Timeline controls"><button onClick={()=>setScale(s=>Math.min(8,s*1.25))}>Zoom in</button><button onClick={()=>setScale(s=>Math.max(.25,s/1.25))}>Zoom out</button><button onClick={()=>{setScale(1);setPan({x:0,y:0});}}>Reset view</button></div>
 <svg aria-label="Journey ribbons and durable milestones" role="img" viewBox={`0 0 ${timeline.width} ${Math.max(180,120+lanes.length*66)}`} onPointerDown={e=>{if(e.target===e.currentTarget)e.currentTarget.setPointerCapture(e.pointerId);}} onPointerMove={e=>{if(e.currentTarget.hasPointerCapture(e.pointerId))setPan(p=>({x:p.x+e.movementX,y:p.y+e.movementY}));}}>
 <defs>{lanes.map((lane,i)=><linearGradient key={lane} id={`${uid}-lane-${i}`}><stop offset="0" stopColor={colors[i%colors.length]} stopOpacity={.9}/><stop offset="1" stopColor={colors[i%colors.length]} stopOpacity={.7}/></linearGradient>)}</defs>
 <g transform={`translate(${pan.x},${pan.y}) scale(${scale})`}>
 {timeline.idle.map((gap,i)=><rect key={i} x={160+gap.from*timeline.scale} y={50} width={(gap.to-gap.from)*timeline.scale} height={80+lanes.length*66} opacity={.12} fill="#8b939f"><title>{`Compressed idle ${gap.ms}ms; chronology unchanged`}</title></rect>)}
 {lanes.map((lane,i)=>{const nodes=snapshot.nodes.filter(n=>n.laneId===lane).sort((a,b)=>a.timestamp-b.timestamp),y=100+i*66;const pts=nodes.map(n=>({x:timeline.at(n.timestamp),y,w:8}));return <g key={lane}><rect x={0} y={y-25} width={timeline.width} height={50} fill={colors[i%colors.length]} opacity={.07}/><text x={8} y={y}>{lane}</text><path data-ribbon={lane} d={ribbonPath(sample(pts))} fill={`url(#${uid}-lane-${i})`}/>{nodes.map(n=><g key={n.id} transform={`translate(${timeline.at(n.timestamp)},${y})`} tabIndex={0} role="button" aria-label={`${n.kind}: ${n.title}`} onClick={()=>pick(n)} onKeyDown={e=>{if(e.key==='Enter'||e.key===' '){e.preventDefault();pick(n);}}}><title>{n.title}</title><Marker kind={n.kind} color={colors[i%colors.length]}/></g>)}</g>;})}
 </g></svg>
 <div aria-label="Milestone legend">{kinds.map(kind=><span key={kind}>{kind} </span>)}</div>
 <ul aria-label="Durable branch tree">{lanes.map(lane=><li key={lane}><button aria-expanded={!collapsed.has(lane)} onClick={()=>setCollapsed(s=>{const n=new Set(s);if(n.has(lane))n.delete(lane);else n.add(lane);return n;})}>{lane}</button>{!collapsed.has(lane)&&<ul>{snapshot.nodes.filter(n=>n.laneId===lane).map(n=><li key={n.id}><button aria-pressed={selected===n.id} onClick={()=>pick(n)}>{n.title}</button></li>)}</ul>}</li>)}</ul>
 <ul aria-label="Persisted lineage facts">{snapshot.edges.filter(e=>e.evidenceId).map(e=><li key={`${e.evidenceId}:${e.from}:${e.to}`}>{e.kind}: {e.from} → {e.to}</li>)}</ul>
 </section>;
}
