// Adapter: ORG2 canonical Journey snapshot -> context-visualizer (MIT, Zaheer 2026, bfd2b50d) events.json-shaped model.
// No model calls, no distillation: every event is a durable node or a durable domain record.
import type {JourneySnapshot,MarkerKind} from "./types";
export interface DomainLike{active_branch_id?:string;branches?:Record<string,{id:string;name:string;parent_branch_id?:string|null;parent_anchor_message_id?:string|null;state:string}>;tasks?:Record<string,{id:string;name:string;branch_id:string;state:string;outcome?:string}>;facts?:Record<string,{id:string;text:string;review_id:string}>;reviews?:Record<string,{id:string;fork_id:string;state:string;annotation?:string}>}
// seq: layout-only order slot after `ts` for derived events that have no timestamp of their own (fork=0, review/close=1).
// Such events carry the real anchor time in `ts`; renderers must not turn `seq` into a time.
export interface SEvent{id:string;thread:string;ts:number;type:MarkerKind;title:string;summary:string;anchorId?:string;handoffTo?:string;seq?:number}
export interface SThread{id:string;name:string;color:string;parent:string|null;fade:boolean;glow:boolean}
export interface SModel{threads:SThread[];events:SEvent[];coverage:{nodes:number;shown:number;hidden:number};activeThread?:string}
// Light per-thread palette (A scheme; tones follow MIT context-visualizer index.html thread colors, lightened).
export const COLORS=["#9db4ff","#f2cf86","#f4a9c4","#a9e0b8","#c3b5ff","#8fd6e0","#f6b38f","#d7e39a","#b9c6d6","#e7a8e8"];
/** Model-written per-node storyline event (same call as the title): topic thread assignment, ≤6-char title, summary. */
export interface NodeTopic{title:string;thread:string;parentThread?:string;threadName?:string;summary?:string}
export const UNTITLED="（待生成）";
export function shortTitle(raw:string,max=16):string{const s=raw.split(/\s*补充说明[:：]/)[0].replace(/\s+/g," ").trim();return s.length>max?s.slice(0,max)+"…":s;}
export function toStoryline(snap:JourneySnapshot,domain:DomainLike|null,titles?:Record<string,string>,topics?:Record<string,NodeTopic>):SModel{
 const nodes=[...snap.nodes].sort((a,b)=>a.timestamp-b.timestamp);
 const mainId="main";const threads:SThread[]=[{id:mainId,name:domain?.branches?.main?.name??"主干",color:COLORS[0],parent:null,fade:false,glow:false}];
 // Topic threads (port of context-visualizer distill-core.js ensureThread, MIT): first-seen order; a new thread forks
 // from its model-declared parentThread when that thread already exists, otherwise it is a root. Nodes without a model
 // topic stay on the main thread (untitled) — nothing is guessed.
 const tid=(t:string)=>"topic:"+t;let mainUsed=false;
 const events:SEvent[]=nodes.map(n=>{const tp=topics?.[n.id];
  if(tp?.thread){const id=tid(tp.thread);if(!threads.some(t=>t.id===id)){const pid=tp.parentThread?tid(tp.parentThread):null;threads.push({id,name:(tp.threadName||tp.thread).slice(0,8),color:COLORS[threads.length%COLORS.length],parent:pid&&threads.some(t=>t.id===pid)?pid:null,fade:false,glow:false});}
   return{id:n.id,thread:id,ts:n.timestamp,type:n.kind,title:tp.title||titles?.[n.id]||UNTITLED,summary:tp.summary||n.title,anchorId:n.anchor.durableMessageId};}
  mainUsed=true;return{id:n.id,thread:mainId,ts:n.timestamp,type:n.kind,title:titles?.[n.id]??UNTITLED,summary:n.title,anchorId:n.anchor.durableMessageId};});
 if(!mainUsed&&threads.length>1&&!Object.keys(domain?.branches??{}).some(k=>k!==mainId)){threads.shift();threads.forEach((t,i)=>t.color=COLORS[i%COLORS.length]);}
 const byMsg=new Map(nodes.map(n=>[n.anchor.durableMessageId,n]));
 const branches=Object.values(domain?.branches??{}).filter(b=>b.id!==mainId);
 branches.forEach((b)=>{
  const parent=b.parent_branch_id&&b.parent_branch_id!==b.id?b.parent_branch_id:mainId;if(!threads.some(t=>t.id===mainId))threads.unshift({id:mainId,name:"主干",color:COLORS[0],parent:null,fade:false,glow:false});
  const at=b.parent_anchor_message_id?byMsg.get(b.parent_anchor_message_id):undefined;if(!at)return;
  const closed=b.state!=="active";threads.push({id:b.id,name:b.name,color:COLORS[(threads.length)%COLORS.length],parent:threads.some(t=>t.id===parent)?parent:mainId,fade:closed,glow:false});
  // ts = the durable anchor's real time; review/close records have no timestamp, so they only get an order slot (seq)
  events.push({id:"fork:"+b.id,thread:b.id,ts:at.timestamp,seq:0,type:"pivot",title:b.name,summary:"从此处分叉："+b.name});
  const rv=Object.values(domain?.reviews??{}).find(r=>r.fork_id===b.id);
  if(rv)events.push({id:"review:"+rv.id,thread:b.id,ts:at.timestamp,seq:1,type:rv.state==="confirmed"?"verify":"finding",title:"复核 "+rv.state,summary:rv.annotation??""});
  if(closed&&!rv)events.push({id:"close:"+b.id,thread:b.id,ts:at.timestamp,seq:1,type:"abandon",title:"已关闭",summary:b.name+" closed"});
 });
 events.sort((a,b)=>a.ts-b.ts||(a.seq??-1)-(b.seq??-1));
 const act=domain?.active_branch_id;
 return{threads,events,coverage:{nodes:nodes.length,shown:nodes.length,hidden:0},activeThread:act&&threads.some(t=>t.id===act)?act:threads.some(t=>t.id===mainId)?(events.filter(e=>e.seq===undefined).at(-1)?.thread??mainId):events.at(-1)?.thread};
}
/** The session's present position: latest timestamped event on the active thread, else the latest timestamped event. */
export function currentEvent(m:SModel):SEvent|undefined{
 const timed=m.events.filter(e=>e.seq===undefined);
 return timed.filter(e=>e.thread===m.activeThread).at(-1)??timed.at(-1);
}
/** Human time text; derived events show their anchor time and say they have no timestamp of their own. */
export function timeText(e:SEvent,fmt:(t:number)=>string):string{return e.seq===undefined?fmt(e.ts):`锚点 ${fmt(e.ts)} 之后（第 ${e.seq+1} 步；无独立时间戳）`;}

// ---- label collision layout (pure; used by JourneyStoryline) ----
export interface Box{x0:number;y0:number;x1:number;y1:number}
export interface LabelReq{id:string;x:number;y:number;text:string;priority:number}
export interface LabelPlace{id:string;text:string;full:string;truncated:boolean;hidden:boolean;x:number;y:number;dy:number;box?:Box}
export interface LabelOpts{fontSize:number;offsets:number[];maxWidth:number;minChars?:number;obstacles?:Box[];bounds?:{x0:number;x1:number}}
const WIDE=/[\u1100-\u115f\u2e80-\ua4cf\uac00-\ud7a3\uf900-\ufaff\ufe30-\ufe4f\uff00-\uff60\uffe0-\uffe6]/;
/** Deterministic width estimate (no DOM): CJK/fullwidth = 1em, other = .6em. Slightly generous so real glyphs fit. */
export function textWidth(s:string,fs:number):number{let w=0;for(const ch of s)w+=WIDE.test(ch)||ch.codePointAt(0)!>0xffff?fs:ch===" "?fs*.32:fs*.6;return w;}
export function fitText(s:string,maxW:number,fs:number):string{
 if(textWidth(s,fs)<=maxW)return s;const cs=[...s];let lo=0,hi=cs.length;
 while(lo<hi){const mid=(lo+hi+1)>>1;if(textWidth(cs.slice(0,mid).join("")+"…",fs)<=maxW)lo=mid;else hi=mid-1;}
 return cs.slice(0,lo).join("")+"…";
}
export const overlaps=(a:Box,b:Box)=>a.x0<b.x1&&b.x0<a.x1&&a.y0<b.y1&&b.y0<a.y1;
/**
 * Greedy collision-free placement. Higher priority first; each label tries its full text in every row offset and a small
 * horizontal nudge, then progressively shorter "…" truncations, and is hidden (tooltip only) when nothing fits.
 * Guarantee: no two visible boxes overlap and no visible box overlaps an obstacle.
 */
export function layoutLabels(reqs:LabelReq[],o:LabelOpts):LabelPlace[]{
 const fs=o.fontSize,pad=3,placed:Box[]=[],obs=o.obstacles??[],out=new Map<string,LabelPlace>();
 const order=reqs.map((r,i)=>({r,i})).sort((a,b)=>b.r.priority-a.r.priority||a.r.x-b.r.x||a.i-b.i);
 for(const {r} of order){
  const full=r.text,n=[...full].length,minC=Math.min(o.minChars??3,n);
  const tiers:string[]=[];const first=fitText(full,o.maxWidth,fs);tiers.push(first);
  const kept=first===full?n:[...first].length-1;
  for(let c=Math.floor(kept*.72);;c=Math.floor(c*.72)){const k=Math.max(c,minC);if(k>=kept||tiers.length>6)break;tiers.push([...full].slice(0,k).join("")+"…");if(k<=minC)break;}
  let got:LabelPlace|undefined;
  for(const text of tiers){
   const w=textWidth(text,fs);
   for(const dy of o.offsets){
    for(const nx of [0,.3,-.3]){
     let cx=r.x+nx*w;
     if(o.bounds)cx=Math.min(Math.max(cx,o.bounds.x0+w/2+pad),o.bounds.x1-w/2-pad);
     const by=r.y+dy,b={x0:cx-w/2-pad,x1:cx+w/2+pad,y0:by-fs*.92,y1:by+fs*.28};
     if(placed.some(p=>overlaps(p,b))||obs.some(p=>overlaps(p,b)))continue;
     got={id:r.id,text,full,truncated:text!==full,hidden:false,x:cx,y:by,dy,box:b};break;
    }
    if(got)break;
   }
   if(got)break;
  }
  if(got)placed.push(got.box!);
  out.set(r.id,got??{id:r.id,text:"",full,truncated:true,hidden:true,x:r.x,y:r.y,dy:0});
 }
 return reqs.map(r=>out.get(r.id)!);
}
export const TYPE_NAME:Record<MarkerKind,string>={question:"提问",finding:"发现",artifact:"产出",decision:"决策",pivot:"转向",abandon:"废弃",interrupt:"打断",verify:"验收",handoff:"交接"};
