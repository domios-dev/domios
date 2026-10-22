use serde_json::Value;
pub(super) fn text(value:&Value)->String{
    let mut lines=Vec::new();
    let mut pending=value["expanded_calls"].as_array().into_iter().flatten().collect::<Vec<_>>();
    let mut count=0;
    while let Some(call)=pending.pop(){
        if count>=64{lines.push("Further call expansion is bounded".into());break;}count+=1;
        lines.push(format!("Call {} -> {}@{}",call["node"].as_str().unwrap_or("?"),call["pin"]["id"].as_str().unwrap_or("?"),call["pin"]["hash"].as_str().unwrap_or("?")));
        if let Ok(graph)=serde_json::to_string_pretty(&call["graph"]){lines.push(graph);}
        pending.extend(call["children"].as_array().into_iter().flatten());
    }
    for node in value["nested_run"]["nodes"].as_array().into_iter().flatten().take(66){
        let stack=node["call_stack"].as_array().into_iter().flatten().map(|frame|format!("{}:{}@{}",frame["node"].as_str().unwrap_or("?"),frame["id"].as_str().unwrap_or("?"),frame["hash"].as_str().unwrap_or("?"))).collect::<Vec<_>>().join(" / ");
        lines.push(format!("Nested {} [{}] result={} stop={} stack={stack}",node["step_name"].as_str().unwrap_or("?"),node["state"].as_str().unwrap_or("?"),node["result_summary"].as_str().unwrap_or("-"),node["reason"].as_str().unwrap_or("-")));
    }
    lines.join("\n")
}
#[cfg(test)]
mod tests;
