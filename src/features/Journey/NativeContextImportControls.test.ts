// @vitest-environment jsdom
import {it,expect,vi} from "vitest";
import {createElement} from "react";
import {createSmokeRoot,dispatch} from "@src/test/reactSmokeHarness";
import {NativeContextImportControls} from "./NativeContextImportControls";
import {invoke} from "@tauri-apps/api/core";
vi.mock("@tauri-apps/api/core",()=>({invoke:vi.fn()}));
const anchor={ownerSessionId:"A",sourceSessionId:"A",branchId:"session:A",durableMessageId:"durable-end",boundaryVersion:7,boundaryHash:"hash"};
const result={snapshotId:"persisted-card",targetSessionId:"A",sourceKind:"session",sourceId:"A",namespace:"session:A",title:"Imported Session Context",tokenEstimate:99,pinned:true,snippet:"prefix",anchor,prefixHash:"prefix"};
it("mounted real controls import only on explicit click and render the native result card",async()=>{
 vi.mocked(invoke).mockReset();vi.mocked(invoke).mockImplementation(async cmd=>cmd==="context_import_snapshots"?[]:result);const root=createSmokeRoot();
 try{await root.render(createElement(NativeContextImportControls,{ownerSessionId:"A",anchor}));expect(invoke).toHaveBeenCalledWith("context_import_snapshots",{ownerSessionId:"A"});const b=root.container.querySelector("button")!;await dispatch(()=>b.click());expect(invoke).toHaveBeenCalledWith("context_import_selected",{ownerSessionId:"A",anchor,tokenBudget:4096,pinned:true});expect(root.container.textContent).toContain("Imported Session Context");expect(root.container.textContent).toContain("session:A");}
 finally{await root.unmount();}
});
it("unmounted/owner-switched pending result cannot contaminate selected session",async()=>{
 vi.mocked(invoke).mockReset();let resolve!:(v:unknown)=>void;vi.mocked(invoke).mockImplementation(cmd=>cmd==="context_import_snapshots"?Promise.resolve([]):new Promise(r=>resolve=r));const root=createSmokeRoot();
 try{await root.render(createElement(NativeContextImportControls,{key:"A",ownerSessionId:"A",anchor}));await dispatch(()=>root.container.querySelector("button")!.click());await root.render(createElement(NativeContextImportControls,{key:"B",ownerSessionId:"B",anchor:null}));await dispatch(()=>resolve(result));expect(root.container.textContent).not.toContain("session:A");expect((root.container.querySelector("button") as HTMLButtonElement).disabled).toBe(true);}
 finally{await root.unmount();}
});

it("remount hydrates immutable native card metadata without import or model execution",async()=>{
 vi.mocked(invoke).mockReset();vi.mocked(invoke).mockResolvedValue([result]);const root=createSmokeRoot();
 try{await root.render(createElement(NativeContextImportControls,{ownerSessionId:"A",anchor:null}));expect(root.container.textContent).toContain("session:A");expect(invoke).toHaveBeenCalledTimes(1);expect(invoke).toHaveBeenCalledWith("context_import_snapshots",{ownerSessionId:"A"});}finally{await root.unmount();}
});
it("pending hydration cannot contaminate unkeyed owner A-B-A selection",async()=>{
 vi.mocked(invoke).mockReset();let resolve!:(value:unknown)=>void;let calls=0;vi.mocked(invoke).mockImplementation(()=>++calls===1?new Promise(r=>resolve=r):Promise.resolve([]));const root=createSmokeRoot();
 try{await root.render(createElement(NativeContextImportControls,{ownerSessionId:"A",anchor:null}));await root.render(createElement(NativeContextImportControls,{ownerSessionId:"B",anchor:null}));await root.render(createElement(NativeContextImportControls,{ownerSessionId:"A",anchor:null}));await dispatch(()=>resolve([result]));expect(root.container.textContent).not.toContain("session:A");}finally{await root.unmount();}
});
