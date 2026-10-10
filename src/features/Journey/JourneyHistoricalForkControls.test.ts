// @vitest-environment jsdom
import {it,expect,vi,beforeEach} from "vitest";
import {createElement} from "react";
import {createSmokeRoot,dispatch} from "@src/test/reactSmokeHarness";
import {invoke} from "@tauri-apps/api/core";
import {JourneyHistoricalForkControls} from "./JourneyHistoricalForkControls";
vi.mock("@tauri-apps/api/core",()=>({invoke:vi.fn()}));
const anchor={ownerSessionId:"A",sourceSessionId:"A",branchId:"session:A",durableMessageId:"answer",boundaryVersion:1,boundaryHash:"hash"};
beforeEach(()=>{vi.mocked(invoke).mockReset();});
it("uses the registered physical prefix contract; retry preserves intent and independent operation renews it",async()=>{
 const root=createSmokeRoot(),done=vi.fn();vi.mocked(invoke).mockRejectedValueOnce("journey_registry_busy").mockResolvedValue({childSessionId:"A-history-child",parentSessionId:"A"});
 try{await root.render(createElement(JourneyHistoricalForkControls,{ownerSessionId:"A",anchor,onCommitted:done}));const click=()=>root.container.querySelector("button")!.click();
 await dispatch(click);const first=vi.mocked(invoke).mock.calls[0];expect(first).toEqual(["historical_runtime_fork",{ownerSessionId:"A",request:{anchor,intentId:expect.any(String)}}]);expect(root.container.querySelector('[role="alert"]')).not.toBeNull();
 await dispatch(click);expect(vi.mocked(invoke).mock.calls[1]).toEqual(first);expect(done).toHaveBeenCalledWith("A-history-child");await dispatch(click);expect(vi.mocked(invoke).mock.calls[2][1]).not.toEqual(first[1]);
 }finally{await root.unmount();}
});
it.each(["owner","unmount"])("drops late authority on %s",async mode=>{
 const root=createSmokeRoot(),done=vi.fn();let resolve!:(v:unknown)=>void;vi.mocked(invoke).mockImplementation(()=>new Promise(r=>{resolve=r}) as never);
 await root.render(createElement(JourneyHistoricalForkControls,{ownerSessionId:"A",anchor,onCommitted:done}));await dispatch(()=>root.container.querySelector("button")!.click());expect(root.container.querySelector("button")!.disabled).toBe(true);
 if(mode==="owner")await root.render(createElement(JourneyHistoricalForkControls,{ownerSessionId:"B",anchor:{...anchor,ownerSessionId:"B",sourceSessionId:"B"},onCommitted:done}));else await root.unmount();
 await dispatch(()=>resolve({childSessionId:"A-history-child",parentSessionId:"A"}));expect(done).not.toHaveBeenCalled();if(mode==="owner")await root.unmount();
});
it("rejects foreign-source anchors without an invoke",async()=>{const root=createSmokeRoot();try{await root.render(createElement(JourneyHistoricalForkControls,{ownerSessionId:"A",anchor:{...anchor,sourceSessionId:"B"},onCommitted:vi.fn()}));expect(root.container.querySelector("button")!.disabled).toBe(true);expect(invoke).not.toHaveBeenCalled();}finally{await root.unmount();}});
