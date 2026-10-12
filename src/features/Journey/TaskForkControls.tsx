import {useCallback,useEffect,useRef,useState} from "react";
import {invoke} from "@tauri-apps/api/core";
import type {DurableAnchor} from "./types";

/** Compressed Task forks: a Task runs in an independent runtime seeded only with a
 *  compressed summary of the main line; finishing it appends ONE source-identified summary
 *  message to the main line. All invariants are enforced natively; this surface only
 *  issues explicit commands and renders durable state (it never invokes a model by itself). */
export interface TaskForkJobView{state:string;outcome:string;attempts:number;error:string|null;deferredReason:string|null;summaryChars:number|null;parentMessageId:string|null;parentSequence:number|null;appendedAfterMessageId:string|null;parentPrefixHashBefore:string|null;parentPrefixHashAfter:string|null;finishSeq:number|null;mergeIndex:number|null}
export interface TaskForkView{id:string;parentSessionId:string;childSessionId:string|null;name:string;state:string;error:string|null;anchorMessageId:string;anchorSequence:number;sourceTokens:number;seedTokens:number|null;modelId:string;job:TaskForkJobView|null;readOnly:boolean;newlyPublished:boolean}

const LINK_LABEL:Record<string,string>={compressing:"🗜️ 压缩中",compress_failed:"😵 压缩失败",active:"🏃 进行中",finishing:"📝 结束中",finished:"✅ 已结束"};
const JOB_LABEL:Record<string,string>={queued:"⏳ 排队中",running:"✍️ 正在总结",summarized:"📦 已总结，等待归并到主线",appended:"🎉 已归并到主线",failed:"😵 总结失败（可重试）",cancelled:"🙅 已取消"};
/** Human wording for the deferral reason; unknown reasons are shown verbatim. */
export function deferredLabel(reason:string):string{
 if(reason.startsWith("merge_order_wait"))return `按结束先后归并：${reason.replace(/^merge_order_wait:\s*/,"")}`;
 if(reason==="parent_busy")return "主线正在处理消息，空闲后自动归并";
 if(reason==="parent_tool_call_open")return "主线有未完成的工具调用，完成后自动归并";
 return reason;
}
const OUTCOMES:Array<[string,string]>=[["completed","完成"],["partially_completed","部分完成"],["paused","暂停"],["abandoned","放弃"],["redirected","转向"]];
const BUSY_JOB=new Set(["queued","running","summarized"]);
const POLL_MS=1500;

export function isTaskForkBusy(view:TaskForkView):boolean{return view.state==="compressing"||(view.job!==null&&BUSY_JOB.has(view.job.state)&&view.state==="finishing");}

export function TaskForkControls({ownerSessionId,anchor,onOpenSession}:{ownerSessionId:string;anchor:DurableAnchor|null;onOpenSession:(sessionId:string)=>void}){
 const [name,setName]=useState(""),[outcome,setOutcome]=useState("completed"),[busy,setBusy]=useState(false),[error,setError]=useState<string|null>(null),[views,setViews]=useState<TaskForkView[]>([]),[tick,setTick]=useState(0);
 // Owner/generation fence: a late response for A must never render under B (or after unmount).
 const scope=useRef({owner:ownerSessionId,generation:0,busy:false});
 if(scope.current.owner!==ownerSessionId){scope.current={owner:ownerSessionId,generation:scope.current.generation+1,busy:false};}
 const intent=useRef<{key:string;id:string}|null>(null);
 useEffect(()=>{setViews([]);setError(null);setBusy(false);setName("");intent.current=null;},[ownerSessionId]);
 useEffect(()=>()=>{scope.current.generation++;scope.current.busy=false;},[]);
 const current=useCallback((generation:number)=>scope.current.owner===ownerSessionId&&scope.current.generation===generation,[ownerSessionId]);

 useEffect(()=>{
  const generation=scope.current.generation;let timer:ReturnType<typeof setTimeout>|undefined;let live=true;
  invoke<TaskForkView[]>("task_fork_list",{ownerSessionId}).then(list=>{
   if(!live||!current(generation))return;
   if(!Array.isArray(list)){setError("task_fork_invalid_response");return;}
   setViews(list.filter(v=>v.parentSessionId===ownerSessionId||v.childSessionId===ownerSessionId));
   if(list.some(isTaskForkBusy))timer=setTimeout(()=>setTick(t=>t+1),POLL_MS);
  },e=>{if(live&&current(generation))setError(String(e));});
  return ()=>{live=false;if(timer)clearTimeout(timer);};
 },[ownerSessionId,tick,current]);

 const run=async(action:()=>Promise<void>)=>{
  if(scope.current.busy)return;
  const generation=scope.current.generation;scope.current.busy=true;setBusy(true);setError(null);
  try{await action();}catch(e){if(current(generation))setError(String(e));}
  finally{if(current(generation)){scope.current.busy=false;setBusy(false);setTick(t=>t+1);}}
 };
 const canCreate=!busy&&name.trim().length>0&&!!anchor&&anchor.ownerSessionId===ownerSessionId&&anchor.sourceSessionId===ownerSessionId;
 const create=()=>run(async()=>{
  if(!canCreate||!anchor)return;
  const generation=scope.current.generation;
  const key=JSON.stringify([anchor,name.trim()]);if(intent.current?.key!==key)intent.current={key,id:crypto.randomUUID()};
  const view=await invoke<TaskForkView>("task_fork_create",{ownerSessionId,request:{intentId:intent.current.id,name:name.trim(),anchor:{...anchor}}});
  if(!current(generation))return;
  if(view.parentSessionId!==ownerSessionId||!view.childSessionId||view.childSessionId===ownerSessionId)throw new Error("task_fork_invalid_response");
  intent.current=null;setName("");onOpenSession(view.childSessionId);
 });
 const finish=(id:string)=>run(async()=>{await invoke("task_fork_finish",{ownerSessionId,request:{taskForkId:id,outcome}});});
 const cancel=(id:string)=>run(async()=>{await invoke("task_fork_cancel_finish",{ownerSessionId,taskForkId:id});});

 return <section aria-label="Task 分叉（压缩上下文）" data-testid="task-fork-controls">
  <strong>Task（压缩上下文的独立分叉）</strong>
  <p>Task 只携带主线的压缩摘要；结束后仅向主线追加一条带来源的摘要消息，不会把 Task 全部对话注入主线。</p>
  <label>Task 名称<input aria-label="Task 名称" data-testid="task-fork-name" value={name} onChange={e=>setName(e.target.value)}/></label>
  <button type="button" data-testid="task-fork-create" disabled={!canCreate} onClick={()=>void create()}>从所选端点新建 Task（先压缩）</button>
  {!anchor&&<p>先在上方选择一个明确的历史端点。</p>}
  {error&&<p role="alert" data-testid="task-fork-error">(；´д｀) 没成功：{error}</p>}
  <ul>{views.map(v=>{
   const isParent=v.parentSessionId===ownerSessionId;const job=v.job;
   const canFinish=!busy&&(v.state==="active"||(v.state==="finishing"&&job!==null&&(job.state==="failed"||job.state==="cancelled")));
   const canCancel=!busy&&v.state==="finishing"&&job!==null&&job.state!=="appended";
   return <li key={v.id} data-testid={`task-fork-row-${v.id}`}>
    <span><strong>{v.name}</strong>：<span data-testid={`task-fork-state-${v.id}`}>{LINK_LABEL[v.state]??v.state}</span>{job&&<> / <span data-testid={`task-fork-job-${v.id}`}>{JOB_LABEL[job.state]??job.state}</span></>}{v.readOnly&&<> / <span data-testid={`task-fork-readonly-${v.id}`}>{v.state==="finished"?"只读（已结束）":"只读（结束中，取消结束可恢复）"}</span></>}</span>
    <div>起点：主线消息 {v.anchorMessageId}（序号 {v.anchorSequence}）；压缩：{v.sourceTokens} → {v.seedTokens??"—"} tokens；模型 {v.modelId}</div>
    {v.error&&<p role="alert">{v.error}</p>}
    {job?.error&&<p role="alert" data-testid={`task-fork-job-error-${v.id}`}>(；´д｀) 总结失败：{job.error}（Task 已保留，可重试）</p>}
    {job?.finishSeq!=null&&job.state!=="cancelled"&&<div data-testid={`task-fork-order-${v.id}`}>结束序 {job.finishSeq}{job.mergeIndex!=null&&<>；第 {job.mergeIndex} 个归并到主线</>}</div>}
    {job?.deferredReason&&job.state!=="appended"&&<p role="status" data-testid={`task-fork-deferred-${v.id}`}>等待条件：{deferredLabel(job.deferredReason)}</p>}
    {job?.state==="appended"&&<p role="status" data-testid={`task-fork-receipt-${v.id}`}>(ﾉ◕ヮ◕)ﾉ 已追加到主线消息 {job.parentMessageId}（序号 {job.parentSequence}），位于 {job.appendedAfterMessageId} 之后；摘要 {job.summaryChars} 字</p>}
    {v.childSessionId&&isParent&&<button type="button" onClick={()=>onOpenSession(v.childSessionId!)}>打开 Task 会话</button>}
    {!isParent&&<button type="button" onClick={()=>onOpenSession(v.parentSessionId)}>回到主线</button>}
    <select aria-label="Task 结果" value={outcome} onChange={e=>setOutcome(e.target.value)}>{OUTCOMES.map(([value,label])=><option key={value} value={value}>{label}</option>)}</select>
    <button type="button" data-testid={`task-fork-finish-${v.id}`} disabled={!canFinish} onClick={()=>void finish(v.id)}>{job&&(job.state==="failed"||job.state==="cancelled")?"重试结束并总结":"结束 Task 并总结"}</button>
    <button type="button" data-testid={`task-fork-cancel-${v.id}`} disabled={!canCancel} onClick={()=>void cancel(v.id)}>取消结束</button>
   </li>;})}</ul>
 </section>;
}
