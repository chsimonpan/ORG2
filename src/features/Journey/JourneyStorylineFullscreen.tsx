import {createPortal} from "react-dom";
import {useEffect,useState} from "react";
import {JourneyStoryline} from "./JourneyStoryline";
import type {DomainLike,NodeTopic} from "./storylineModel";
import type {DurableAnchor,JourneySnapshot} from "./types";
/** Full-page browsing surface for the storyline. Read-only; Esc or the close button dismisses it.
 *  Uses the "full" layout (taller lanes, more label rows, wider time span, larger type) instead of scaling the inline SVG. */
export function JourneyStorylineFullscreen({snapshot,domain,titles,topics,onSelect,onClose}:{snapshot:JourneySnapshot;domain:DomainLike|null;titles?:Record<string,string>;topics?:Record<string,NodeTopic>;onSelect:(a:DurableAnchor)=>void;onClose:()=>void}){
 useEffect(()=>{const k=(e:KeyboardEvent)=>{if(e.key==="Escape"){e.stopPropagation();onClose();}};window.addEventListener("keydown",k,true);return()=>window.removeEventListener("keydown",k,true);},[onClose]);
 const [vw]=useState(()=>typeof window==="undefined"?1440:window.innerWidth);
 return createPortal(<div role="dialog" aria-modal="true" aria-label="Journey storyline full view" style={{position:"fixed",inset:0,zIndex:1000,background:"#0b1020",display:"flex",flexDirection:"column"}}>
  <header style={{display:"flex",alignItems:"center",gap:12,padding:"10px 16px",borderBottom:"1px solid #1f2a40",color:"#e6e9f2"}}>
   <strong style={{fontSize:15}}>Journey 故事线 · 全图</strong>
   <span style={{color:"#8a94a8",fontSize:12}}>横轴为真实时间（夜间压缩）· 拖动空白处平移 · +/- 横向缩放，标签自动重新避让</span>
   <button type="button" onClick={onClose} style={{marginLeft:"auto",padding:"4px 12px",borderRadius:6,background:"#1b2233",color:"#e6e9f2",border:"1px solid #33405e"}}>关闭全图 (Esc)</button>
  </header>
  <div style={{flex:1,overflow:"auto",padding:16}}>
   <JourneyStoryline snapshot={snapshot} domain={domain} titles={titles} topics={topics} onSelect={onSelect} variant="full" fitWidth={vw-64}/>
  </div>
 </div>,document.body);
}
