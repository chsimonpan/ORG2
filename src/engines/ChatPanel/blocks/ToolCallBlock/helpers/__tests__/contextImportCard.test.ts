import React from "react";
import {readFileSync} from "node:fs";
import {renderToStaticMarkup} from "react-dom/server";
import {describe,expect,it} from "vitest";
import {parseContextImportCardResult} from "../cardParsers";
import ContextImportCard from "../../cards/ContextImportCard";
describe("historical import_context production tool card",()=>{
 it("preserves returned source snapshot namespace pinned and finite tokens",()=>{
  expect(parseContextImportCardResult({source_kind:"task",source_id:"old",pinned:false},{source_kind:"session",source_id:"actual",snapshotId:"snap",namespace:"session:actual",title:"Imported",tokenEstimate:23.9,pinned:true})).toEqual({sourceKind:"session",sourceId:"actual",snapshotId:"snap",namespace:"session:actual",title:"Imported",tokenEstimate:23,pinned:true});
 });
 it("rejects unidentified source and nonfinite tokens",()=>{
  expect(parseContextImportCardResult({},{})).toBeNull();
  expect(parseContextImportCardResult({source_kind:"task"},{})).toBeNull();
  expect(parseContextImportCardResult({source_kind:"task",source_id:"t1"},{token_estimate:Infinity})?.tokenEstimate).toBeUndefined();
  expect(parseContextImportCardResult({source_kind:"task",source_id:"t1",pinned:false},{token_estimate:-2})).toMatchObject({namespace:"task:t1",pinned:false,tokenEstimate:0});
 });
 it("renders escaped title snapshot namespace and pinned state",()=>{
  const html=renderToStaticMarkup(React.createElement(ContextImportCard,{card:{sourceKind:"session",sourceId:"s1",namespace:"session:s1",snapshotId:"snap",title:"<script>bad</script>",tokenEstimate:12,pinned:true}}));
  expect(html).toContain("&lt;script&gt;bad&lt;/script&gt;");expect(html).not.toContain("<script>bad");expect(html).toContain("snap");expect(html).toContain("session:s1");expect(html).toContain("12 tokens");expect(html).toContain("Pinned");
 });
 it("uses production dispatch and output renderer",()=>{
  const root="src/engines/ChatPanel/blocks/ToolCallBlock/";
  expect(readFileSync(root+"index.tsx","utf8")).toMatch(/toolName === "import_context" && hasResult && !isError/);
  expect(readFileSync(root+"OutputContent.tsx","utf8")).toContain('styledOutput?.type === "contextImportCard"');
 });
});
