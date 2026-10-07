import {useEffect,useRef,useState} from "react";import {invoke} from "@tauri-apps/api/core";
import type {DurableAnchor} from "./types";import type {JourneyDomainSnapshot} from "./nativeAdapter";
/** Explicit user-driven actual lifecycle commands. Rendering does not mutate or invoke a model. */
export function JourneyLifecycleControls({ownerSessionId,domain,anchor,onCommitted}:{ownerSessionId:string;domain:JourneyDomainSnapshot;anchor:DurableAnchor|null;onCommitted:()=>void}){
 const [name,setName]=useState(""),[pending,setPending]=useState(false),[error,setError]=useState<string|null>(null);
 const scope=useRef({owner:ownerSessionId,generation:0,request:0});if(scope.current.owner!==ownerSessionId){scope.current={owner:ownerSessionId,generation:scope.current.generation+1,request:scope.current.request+1};}
 useEffect(()=>()=>{scope.current.request++;scope.current.generation++;},[]);
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
 return <section aria-label="Journey lifecycle actions"><label>Name<input aria-label="Journey task or fork name" value={name} onChange={e=>setName(e.target.value)}/></label><button disabled={pending||!name.trim()} onClick={()=>void write("task")}>Start named task at next persisted user</button><button disabled={pending||!name.trim()||!anchor} onClick={()=>void write("fork")}>Fork exact selected endpoint</button><button disabled={pending||!name.trim()||!anchor} onClick={()=>void write("checkpoint")}>Checkpoint exact selected endpoint</button>{error&&<p role="alert">{error}</p>}</section>;
}
