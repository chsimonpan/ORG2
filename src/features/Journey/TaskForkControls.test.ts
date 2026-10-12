// @vitest-environment jsdom
import {it,expect,vi,beforeEach} from "vitest";
import {createElement} from "react";
import {createSmokeRoot,dispatch} from "@src/test/reactSmokeHarness";
import {invoke} from "@tauri-apps/api/core";
import {TaskForkControls} from "./TaskForkControls";
import {deferredLabel} from "./TaskForkControls";
import type {TaskForkView} from "./TaskForkControls";
vi.mock("@tauri-apps/api/core",()=>({invoke:vi.fn()}));
const anchor={ownerSessionId:"A",sourceSessionId:"A",branchId:"session:A",durableMessageId:"answer",boundaryVersion:1,boundaryHash:"hash"};
const view=(over:Partial<TaskForkView>={}):TaskForkView=>({id:"tf-1",parentSessionId:"A",childSessionId:"A-task-1",name:"对账",state:"active",error:null,anchorMessageId:"answer",anchorSequence:3,sourceTokens:900,seedTokens:120,modelId:"m",job:null,readOnly:false,newlyPublished:false,...over});
const job=(state:string,extra:Record<string,unknown>={})=>({state,outcome:"completed",attempts:1,error:null,deferredReason:null,summaryChars:null,parentMessageId:null,parentSequence:null,appendedAfterMessageId:null,parentPrefixHashBefore:null,parentPrefixHashAfter:null,finishSeq:null,mergeIndex:null,...extra});
beforeEach(()=>{vi.mocked(invoke).mockReset();});
const typeName=async(root:ReturnType<typeof createSmokeRoot>,text:string)=>{const input=root.container.querySelector<HTMLInputElement>('[data-testid="task-fork-name"]')!;await dispatch(()=>{const setter=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value")!.set!;setter.call(input,text);input.dispatchEvent(new Event("input",{bubbles:true}));});};

it("create keeps one intent across a failed compression retry, then opens the child",async()=>{
 const root=createSmokeRoot(),open=vi.fn();
 vi.mocked(invoke).mockImplementation(async(cmd:string)=>{
  if(cmd==="task_fork_list")return [] as never;
  if(cmd==="task_fork_create"){const n=vi.mocked(invoke).mock.calls.filter(c=>c[0]==="task_fork_create").length;if(n===1)throw "task_fork_summary_empty";return view({newlyPublished:true}) as never;}
  throw new Error(cmd);
 });
 try{
  await root.render(createElement(TaskForkControls,{ownerSessionId:"A",anchor,onOpenSession:open}));
  await typeName(root,"对账");
  const click=()=>root.container.querySelector<HTMLButtonElement>('[data-testid="task-fork-create"]')!.click();
  await dispatch(click);
  const creates=()=>vi.mocked(invoke).mock.calls.filter(c=>c[0]==="task_fork_create");
  expect(creates()[0]).toEqual(["task_fork_create",{ownerSessionId:"A",request:{intentId:expect.any(String),name:"对账",anchor}}]);
  expect(root.container.querySelector('[data-testid="task-fork-error"]')!.textContent).toContain("task_fork_summary_empty");
  expect(open).not.toHaveBeenCalled();
  await dispatch(click);
  expect(creates()[1]).toEqual(creates()[0]);
  expect(open).toHaveBeenCalledWith("A-task-1");
 }finally{await root.unmount();}
});

it("shows durable job states, the append receipt, and exposes retry only for failed jobs",async()=>{
 const root=createSmokeRoot();
 vi.mocked(invoke).mockImplementation(async(cmd:string)=>{
  if(cmd==="task_fork_list")return [view({state:"finishing",job:job("failed",{error:"summary_provider_503"})}),view({id:"tf-2",name:"已归并",state:"finished",job:job("appended",{summaryChars:321,parentMessageId:"task-summary:tf-2",parentSequence:4,appendedAfterMessageId:"answer"})})] as never;
  return view() as never;
 });
 try{
  await root.render(createElement(TaskForkControls,{ownerSessionId:"A",anchor:null,onOpenSession:vi.fn()}));
  const text=root.container.textContent!;
  expect(text).toContain("总结失败（可重试）");expect(text).toContain("summary_provider_503");expect(text).toContain("Task 已保留");
  expect(root.container.querySelector('[data-testid="task-fork-receipt-tf-2"]')!.textContent).toContain("task-summary:tf-2");
  expect(text).toContain("😵 总结失败（可重试）");expect(text).toContain("🎉 已归并到主线");
  const retry=root.container.querySelector<HTMLButtonElement>('[data-testid="task-fork-finish-tf-1"]')!;expect(retry.textContent).toBe("重试结束并总结");expect(retry.disabled).toBe(false);
  expect(root.container.querySelector<HTMLButtonElement>('[data-testid="task-fork-finish-tf-2"]')!.disabled).toBe(true);
  await dispatch(()=>retry.click());
  expect(vi.mocked(invoke)).toHaveBeenCalledWith("task_fork_finish",{ownerSessionId:"A",request:{taskForkId:"tf-1",outcome:"completed"}});
 }finally{await root.unmount();}
});

it("drops late list responses after the owner changes and never shows another owner's tasks",async()=>{
 const root=createSmokeRoot();let resolveA!:(v:unknown)=>void;
 vi.mocked(invoke).mockImplementation(((cmd:string,args:{ownerSessionId:string})=>{
  if(args.ownerSessionId==="A")return new Promise(r=>{resolveA=r});
  return Promise.resolve([]);
 }) as never);
 try{
  await root.render(createElement(TaskForkControls,{ownerSessionId:"A",anchor,onOpenSession:vi.fn()}));
  await root.render(createElement(TaskForkControls,{ownerSessionId:"B",anchor:{...anchor,ownerSessionId:"B",sourceSessionId:"B"},onOpenSession:vi.fn()}));
  await dispatch(()=>resolveA([view({name:"属于A的Task"})]));
  expect(root.container.textContent).not.toContain("属于A的Task");
 }finally{await root.unmount();}
});

it("refuses a foreign-source anchor without invoking create",async()=>{
 const root=createSmokeRoot();vi.mocked(invoke).mockResolvedValue([] as never);
 try{
  await root.render(createElement(TaskForkControls,{ownerSessionId:"A",anchor:{...anchor,sourceSessionId:"B"},onOpenSession:vi.fn()}));
  await typeName(root,"x");
  expect(root.container.querySelector<HTMLButtonElement>('[data-testid="task-fork-create"]')!.disabled).toBe(true);
  expect(vi.mocked(invoke).mock.calls.some(c=>c[0]==="task_fork_create")).toBe(false);
 }finally{await root.unmount();}
});

it("labels read-only Tasks and explains the merge order and waiting reason",async()=>{
 const root=createSmokeRoot();
 vi.mocked(invoke).mockImplementation(async(cmd:string)=>{
  if(cmd==="task_fork_list")return [
   view({id:"tf-1",name:"先结束",state:"finished",readOnly:true,job:job("appended",{finishSeq:1,mergeIndex:1,summaryChars:10,parentMessageId:"task-summary:tf-1",parentSequence:4,appendedAfterMessageId:"answer"})}),
   view({id:"tf-2",name:"后结束",state:"finishing",readOnly:true,job:job("summarized",{finishSeq:2,deferredReason:"merge_order_wait: 「后结束」（结束序 2）等待更早结束的 Task「先结束」（结束序 1）先归并"})}),
   view({id:"tf-3",name:"进行中",state:"active"}),
  ] as never;
  return view() as never;
 });
 try{
  await root.render(createElement(TaskForkControls,{ownerSessionId:"A",anchor:null,onOpenSession:vi.fn()}));
  const q=(id:string)=>root.container.querySelector(`[data-testid="${id}"]`);
  expect(q("task-fork-readonly-tf-1")!.textContent).toBe("只读（已结束）");
  expect(q("task-fork-readonly-tf-2")!.textContent).toContain("取消结束可恢复");
  expect(q("task-fork-readonly-tf-3")).toBeNull();
  expect(q("task-fork-order-tf-1")!.textContent).toBe("结束序 1；第 1 个归并到主线");
  expect(q("task-fork-order-tf-2")!.textContent).toBe("结束序 2");
  expect(q("task-fork-deferred-tf-2")!.textContent).toContain("按结束先后归并");
  expect(q("task-fork-deferred-tf-2")!.textContent).toContain("等待更早结束的 Task「先结束」");
 }finally{await root.unmount();}
});

it("deferredLabel keeps unknown reasons verbatim",()=>{
 expect(deferredLabel("parent_busy")).toContain("空闲后自动归并");
 expect(deferredLabel("parent_tool_call_open")).toContain("工具调用");
 expect(deferredLabel("weird_reason")).toBe("weird_reason");
});
