import {NativeContextImportControls} from "./NativeContextImportControls";
// Licensed Journey canvas is a read-only canonical session surface. Never performs model work.
import {useEffect,useRef,useState} from "react";
import {JourneyCanvas} from "./JourneyCanvas";
import {JourneyLifecycleControls} from "./JourneyLifecycleControls";
import {loadJourneySnapshot,loadJourneyDomainSnapshot,loadJourneyBoundaryPreview,openDurableJourneyOwner} from "./nativeAdapter";
import type {JourneyDomainSnapshot,BoundaryPreview} from "./nativeAdapter";
import {validateAnchor} from "./canonicalScope";
import type {DurableAnchor,JourneySnapshot} from "./types";
export function JourneyPopover({ownerSessionId}:{ownerSessionId:string}){
 const [open,setOpen]=useState(false),[opening,setOpening]=useState(false),[openError,setOpenError]=useState<string|null>(null);
 const openRequest=useRef(0);useEffect(()=>()=>{openRequest.current++;},[ownerSessionId]);
 const toggle=async()=>{if(open){setOpen(false);return;}if(opening)return;const request=++openRequest.current;setOpening(true);setOpenError(null);try{await openDurableJourneyOwner(ownerSessionId);if(openRequest.current===request)setOpen(true);}catch(error){if(openRequest.current===request)setOpenError(String(error));}finally{if(openRequest.current===request)setOpening(false);}};
 return <><button type="button" aria-label="Session Journey" aria-expanded={open} disabled={opening} onClick={()=>void toggle()}>Journey</button>{openError&&<p role="alert">{openError}</p>}{open&&<JourneyOwnerPanel key={ownerSessionId} ownerSessionId={ownerSessionId} onClose={()=>setOpen(false)}/>}</>;
}
export function JourneyOwnerPanel({ownerSessionId,onClose}:{ownerSessionId:string;onClose:()=>void}){
 const [snapshot,setSnapshot]=useState<JourneySnapshot|null>(null),[error,setError]=useState<string|null>(null),[selected,setSelected]=useState<DurableAnchor|null>(null),[reload,setReload]=useState(0);
 const [domain,setDomain]=useState<JourneyDomainSnapshot|null>(null),[preview,setPreview]=useState<BoundaryPreview|null>(null),[previewError,setPreviewError]=useState<string|null>(null);const previewRequest=useRef(0);
 const scope=useRef({owner:ownerSessionId,generation:0,request:0});
 // Synchronous invalidation precedes effects. A-B-A cannot reuse A1 request authority.
 if(scope.current.owner!==ownerSessionId){scope.current={owner:ownerSessionId,generation:scope.current.generation+1,request:scope.current.request+1};}
 useEffect(()=>{
  const s=scope.current;const generation=s.generation,request=++s.request;let live=true;
  setSnapshot(null);setSelected(null);setError(null);setDomain(null);setPreview(null);setPreviewError(null);previewRequest.current++;
  const current=()=>live&&scope.current.owner===ownerSessionId&&scope.current.generation===generation&&scope.current.request===request;
  loadJourneySnapshot(ownerSessionId).then(result=>{if(current())setSnapshot(result);},e=>{if(current())setError(String(e));});
  loadJourneyDomainSnapshot(ownerSessionId).then(result=>{if(current())setDomain(result);},e=>{if(current())setError(String(e));});
  return ()=>{live=false;previewRequest.current++;};
 },[ownerSessionId,reload]);
 const visible=snapshot?.ownerSessionId===ownerSessionId?snapshot:null;
 const active=selected?.ownerSessionId===ownerSessionId&&visible?.nodes.some(n=>n.anchor.boundaryHash===selected.boundaryHash)?selected:null;
 const requestPreview=async()=>{if(!active)return;const anchor=active,s=scope.current,generation=s.generation,request=++previewRequest.current;setPreview(null);setPreviewError(null);const current=()=>scope.current.owner===ownerSessionId&&scope.current.generation===generation&&previewRequest.current===request;try{const result=await loadJourneyBoundaryPreview(ownerSessionId,anchor);if(current())setPreview(result);}catch(e){if(current())setPreviewError(String(e));}};
 const pick=(a:DurableAnchor)=>{validateAnchor(a);if(a.ownerSessionId!==ownerSessionId||!visible?.nodes.some(n=>n.anchor.boundaryHash===a.boundaryHash&&n.anchor.durableMessageId===a.durableMessageId))throw Error("Journey explicit anchor unavailable");previewRequest.current++;setPreview(null);setPreviewError(null);setSelected(a);};
 return <section role="dialog" aria-label="Canonical Session Journey" className="absolute right-0 top-full z-50 max-h-[80vh] w-[min(900px,95vw)] overflow-auto rounded-lg border border-border-2 bg-chat-container p-3 text-text-1 shadow-lg" onKeyDown={e=>{if(e.key==="Escape")onClose();}}>
  <header><strong>Session Journey</strong><button type="button" onClick={()=>setReload(v=>v+1)}>Refresh Journey</button><button type="button" onClick={onClose}>Close Journey</button></header>
  <p>Canonical durable history. Read-only; preview, import, fork, facts and reviews require independently validated service permissions.</p>
  <NativeContextImportControls key={ownerSessionId+":import"} ownerSessionId={ownerSessionId} anchor={active}/>
  {domain?.snapshot.session_id===ownerSessionId&&<aside aria-label="Journey branch tree"><strong>Branches / tasks</strong><p>Active branch: {domain.snapshot.active_branch_id}</p><ul>{Object.values(domain.snapshot.branches).map(f=><li key={f.id}>{f.name||f.id}: {f.state} / parent {f.parent_branch_id} / exact endpoint {f.parent_anchor_message_id||"unavailable"}</li>)}</ul><ul>{Object.values(domain.snapshot.tasks).map(task=><li key={task.id}>{task.name}: {task.state} / branch {task.branch_id}</li>)}</ul><p>{Object.keys(domain.snapshot.facts).length} facts / {Object.keys(domain.snapshot.reviews).length} review items</p></aside>}
  {domain?.snapshot.session_id===ownerSessionId&&<JourneyLifecycleControls key={ownerSessionId} ownerSessionId={ownerSessionId} domain={domain} anchor={active} onCommitted={()=>setReload(v=>v+1)}/>}
  <button type="button" disabled={!active} onClick={()=>void requestPreview()}>Preview exact historical prefix</button>{previewError&&<p role="alert">{previewError}</p>}{preview?.ownerSessionId===ownerSessionId&&preview.anchor.boundaryHash===active?.boundaryHash&&<article aria-label="Journey exact prefix preview"><p>Prefix hash: {preview.prefixHash}</p>{preview.records.map(row=><section key={row.durableMessageId}><strong>{row.role}: {row.durableMessageId}</strong><pre className="whitespace-pre-wrap">{row.content}</pre></section>)}</article>}
  {error&&<p role="alert">{error}</p>}{!visible&&!error&&<p role="status">Loading canonical Journey</p>}
  {visible&&<><JourneyCanvas snapshot={visible} onSelect={pick}/><label>Historical endpoint<select aria-label="Journey historical endpoint" value={active?.durableMessageId??""} onChange={e=>{const n=visible.nodes.find(n=>n.anchor.durableMessageId===e.target.value);if(n)pick(n.anchor);else setSelected(null);}}><option value="">Select explicit durable endpoint</option>{visible.nodes.map(n=><option key={n.id} value={n.anchor.durableMessageId}>{n.kind}: {n.title}</option>)}</select></label><p>{visible.nodes.length} persisted milestones / revision {visible.revision}</p>{active&&<dl data-testid="journey-explicit-endpoint"><dt>Durable endpoint</dt><dd>{active.durableMessageId}</dd><dt>Source branch</dt><dd>{active.branchId}</dd><dt>Boundary hash</dt><dd>{active.boundaryHash}</dd></dl>}{visible.nodes.length===0&&<p>No persisted endpoints</p>}</>}
 </section>;
}
