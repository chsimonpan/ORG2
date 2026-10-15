import {useEffect,useRef,useState} from "react";import {invoke} from "@tauri-apps/api/core";
import type {DurableAnchor} from "./types";import type {JourneyDomainSnapshot} from "./nativeAdapter";
/** Explicit user-driven actual lifecycle commands. Rendering does not mutate or invoke a model. */
export function JourneyLifecycleControls({ownerSessionId,domain,anchor,onCommitted}:{ownerSessionId:string;domain:JourneyDomainSnapshot;anchor:DurableAnchor|null;onCommitted:()=>void}){
 const [name,setName]=useState(""),[pending,setPending]=useState(false),[error,setError]=useState<string|null>(null);
 const [promote,setPromote]=useState<{reviewId:string;text:string;start:string;end:string}|null>(null);
 const scope=useRef({owner:ownerSessionId,generation:0,request:0});if(scope.current.owner!==ownerSessionId){scope.current={owner:ownerSessionId,generation:scope.current.generation+1,request:scope.current.request+1};setPending(false);setError(null);setName("");}
 useEffect(()=>()=>{scope.current.request++;scope.current.generation++;},[]);
 const endpointWrite=async(kind:"finish"|"activate",task?:{id:string;branch_id:string})=>{
  if(pending||domain.snapshot.session_id!==ownerSessionId)return;
  if(kind==="finish"&&(!anchor||anchor.ownerSessionId!==ownerSessionId))return;
  if(kind==="activate"&&!task)return;
  const s=scope.current,generation=s.generation,request=++s.request;
  const current=()=>scope.current.owner===ownerSessionId&&scope.current.generation===generation&&scope.current.request===request;
  setPending(true);setError(null);
  const base={sessionId:ownerSessionId,expectedRevision:domain.revision};
  try{
   if(kind==="finish")await invoke("journey_task_finish",{ownerSessionId,request:{...base,outcome:"completed",messageId:anchor!.durableMessageId}});
   else await invoke("journey_entry_activate",{ownerSessionId,request:{...base,taskId:task!.id,branchId:task!.branch_id}});
   if(current())onCommitted();
  }catch(e){if(current())setError(String(e));}finally{if(current())setPending(false);}
 };
 const write=async(kind:"task"|"fork"|"checkpoint")=>{
  if(pending||!name.trim()||domain.snapshot.session_id!==ownerSessionId)return;
  if(kind!=="task"&&(!anchor||anchor.ownerSessionId!==ownerSessionId))return;
  const s=scope.current,generation=s.generation,request=++s.request;
  const current=()=>scope.current.owner===ownerSessionId&&scope.current.generation===generation&&scope.current.request===request;
  setPending(true);setError(null);
  const id=crypto.randomUUID();const base={sessionId:ownerSessionId,expectedRevision:domain.revision};
  try{if(kind==="task")await invoke("journey_task_start",{ownerSessionId,request:{...base,taskId:id,name:name.trim(),position:"下一条用户消息"}});
   else if(kind==="checkpoint")await invoke("journey_checkpoint",{ownerSessionId,request:{...base,checkpointId:id,name:name.trim(),messageId:anchor!.durableMessageId}});
   else await invoke("journey_fork_start",{ownerSessionId,anchor,request:{...base,forkId:id,taskId:crypto.randomUUID(),taskName:name.trim(),anchorMessageId:anchor!.durableMessageId}});
   if(current())onCommitted();
  }catch(e){if(current())setError(String(e));}finally{if(current())setPending(false);}
 };
 const reviewWrite=async(command:"journey_review_retry"|"journey_return_parent"|"journey_discard",reviewId:string)=>{
  if(pending||domain.snapshot.session_id!==ownerSessionId||!reviewId)return;
  const s=scope.current,generation=s.generation,request=++s.request;
  const current=()=>scope.current.owner===ownerSessionId&&scope.current.generation===generation&&scope.current.request===request;
  setPending(true);setError(null);
  try{await invoke(command,{ownerSessionId,request:{sessionId:ownerSessionId,expectedRevision:domain.revision,reviewId}});if(current())onCommitted();}
  catch(e){if(current())setError(String(e));}finally{if(current())setPending(false);}
 };

 const promoteWrite=async()=>{
  if(pending||!promote||domain.snapshot.session_id!==ownerSessionId)return;
  const text=promote.text.trim(),start=promote.start.trim(),end=promote.end.trim();
  if(!text||!start||!end)return;
  const s=scope.current,generation=s.generation,request=++s.request;
  const current=()=>scope.current.owner===ownerSessionId&&scope.current.generation===generation&&scope.current.request===request;
  setPending(true);setError(null);
  try{await invoke("journey_confirm",{ownerSessionId,request:{sessionId:ownerSessionId,expectedRevision:domain.revision,reviewId:promote.reviewId,factId:crypto.randomUUID(),text,evidenceStartMessageId:start,evidenceEndMessageId:end}});if(current()){setPromote(null);onCommitted();}}
  catch(e){if(current())setError(String(e));}finally{if(current())setPending(false);}
 };
 const reviewControls=Object.values(domain.snapshot.reviews).flatMap(value=>{
  if(!value||typeof value!=="object"||Array.isArray(value))return [];
  const review=value as {id?:unknown;state?:unknown};if(typeof review.id!=="string"||!review.id)return [];
  const id=review.id,disabled=pending||domain.snapshot.session_id!==ownerSessionId;
  if(review.state==="failed")return [<button key={id+":retry"} disabled={disabled} onClick={()=>void reviewWrite("journey_review_retry",id)}>Retry review {id}</button>];
  if(review.state==="ready"||review.state==="confirmed")return [<button key={id+":return"} disabled={disabled} onClick={()=>void reviewWrite("journey_return_parent",id)}>Return to parent {id}</button>,...(review.state==="ready"?[<button key={id+":promote"} disabled={disabled} onClick={()=>setPromote({reviewId:id,text:"",start:"",end:""})}>Promote fact {id}</button>,<button key={id+":discard"} disabled={disabled} onClick={()=>void reviewWrite("journey_discard",id)}>Discard fork {id}</button>]:[])];
  return [];
 });
 return <section aria-label="Journey lifecycle actions"><label>Name<input aria-label="Journey task or fork name" value={name} onChange={e=>setName(e.target.value)}/></label><button disabled={pending||!name.trim()} onClick={()=>void write("task")}>Start named task at next persisted user</button><button disabled={pending||!name.trim()||!anchor} onClick={()=>void write("fork")}>Fork exact selected endpoint</button><button disabled={pending||!name.trim()||!anchor} onClick={()=>void write("checkpoint")}>Checkpoint exact selected endpoint</button><button disabled={pending||!anchor||anchor.ownerSessionId!==ownerSessionId} onClick={()=>void endpointWrite("finish")}>Finish active task at selected endpoint</button>{Object.values(domain.snapshot.tasks).map(task=><button key={task.id} disabled={pending} onClick={()=>void endpointWrite("activate",task)}>Activate {task.name}</button>)}{reviewControls}{promote&&<fieldset aria-label="Promote fact form"><legend>Promote fact from review {promote.reviewId}</legend><label>Fact text<input aria-label="Fact text" value={promote.text} onChange={e=>setPromote({...promote,text:e.target.value})}/></label><label>Evidence start message ID<input aria-label="Evidence start message ID" value={promote.start} onChange={e=>setPromote({...promote,start:e.target.value})}/></label><label>Evidence end message ID<input aria-label="Evidence end message ID" value={promote.end} onChange={e=>setPromote({...promote,end:e.target.value})}/></label><button disabled={pending||!promote.text.trim()||!promote.start.trim()||!promote.end.trim()} onClick={()=>void promoteWrite()}>Confirm promote</button><button disabled={pending} onClick={()=>setPromote(null)}>Cancel promote</button></fieldset>}{error&&<p role="alert">{error}</p>}</section>;
}
