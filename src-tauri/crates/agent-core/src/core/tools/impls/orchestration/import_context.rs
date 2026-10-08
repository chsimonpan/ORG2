//! Native bounded same-owner import, registered in the canonical runtime builder.
use async_trait::async_trait;
use serde_json::{Value,json};
use crate::tools::traits::{Tool,ToolError,CallContext,required_string};
use crate::session::journey_canonical::DurableAnchor;
pub struct ImportContextTool { owner: String }
impl ImportContextTool { pub fn new(owner:String)->Self{Self{owner}} }
#[async_trait]
impl Tool for ImportContextTool {
 fn name(&self)->&str{"import_context"}
 fn category(&self)->&str{crate::tools::categories::SESSION}
 fn description(&self)->&str{"Persist an explicit bounded excerpt from a selected canonical same-owner session end anchor. Cross-session and non-session imports require separately granted adapters and are denied."}
 fn parameters(&self)->Value{json!({"type":"object","additionalProperties":false,"required":["source_kind","source_id","anchor","token_budget"],"properties":{"source_kind":{"type":"string","enum":["session"]},"source_id":{"type":"string"},"anchor":{"type":"object","description":"Exact durable anchor from native Journey selection/preview; not a label or ordinal."},"token_budget":{"type":"integer","minimum":1,"maximum":32768},"title":{"type":"string"},"pinned":{"type":"boolean"}}})}
 async fn execute_text(&self,p:Value,ctx:&CallContext)->Result<String,ToolError>{
  ctx.require_tool_authority(self.name())?;
  if ctx.session_id!=self.owner{return Err(ToolError::ExecutionFailed("context_import_owner_mismatch".into()));}
  let allowed=["source_kind","source_id","anchor","token_budget","title","pinned"];
  if p.as_object().is_none_or(|m|m.keys().any(|k|!allowed.contains(&k.as_str()))){return Err(ToolError::InvalidParams("context_import_unknown_parameter".into()));}
  if required_string(&p,"source_kind")?!="session"{return Err(ToolError::InvalidParams("context_import_source_adapter_not_authorized".into()));}
  let source=required_string(&p,"source_id")?;
  let anchor:DurableAnchor=serde_json::from_value(p.get("anchor").cloned().unwrap_or(Value::Null)).map_err(|e|ToolError::InvalidParams(e.to_string()))?;
  let budget=p.get("token_budget").and_then(Value::as_u64).and_then(|n|usize::try_from(n).ok()).ok_or_else(||ToolError::InvalidParams("context_import_invalid_budget".into()))?;
  let pinned=match p.get("pinned"){Some(Value::Bool(v))=>*v,None=>false,_=>return Err(ToolError::InvalidParams("pinned must be boolean".into()))};
  let title=match p.get("title"){Some(Value::String(s))=>Some(s.as_str()),None=>None,_=>return Err(ToolError::InvalidParams("title must be string".into()))};
  let _writer=database::db::sessions_writer_guard();
  let c=crate::foundation::db_bridge::get_connection().map_err(|e|ToolError::ExecutionFailed(e.to_string()))?;
  let result=crate::session::context_import::import_with_connection(&c,&self.owner,&source,&anchor,budget,title,pinned).map_err(ToolError::ExecutionFailed)?;
  serde_json::to_string(&result).map_err(|e|ToolError::ExecutionFailed(e.to_string()))
 }
}
