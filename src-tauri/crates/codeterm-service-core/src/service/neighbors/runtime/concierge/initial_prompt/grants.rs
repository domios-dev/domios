use super::*;

pub(super) fn capability_grants(friendship: &str, permission: &Permission) -> Value {
    let memory = permission.allowed && permission.allow_memory;
    let files = permission.allowed && permission.allow_files;
    let areas = permission.memory_areas.as_ref();
    let memory_search: Vec<Value> = if !memory {
        vec![]
    } else {
        match areas {
            None => vec![json!(["domios", "mem", "search", "<query>"])],
            Some(areas) => areas
                .iter()
                .map(|area| json!(["domios", "mem", "search", "<query>", "--area", area]))
                .collect(),
        }
    };
    let workspaces = permission.workspaces.as_deref().unwrap_or_default();
    let tab_list: Vec<Value> = workspaces
        .iter()
        .filter(|_| permission.allowed)
        .map(|workspace| json!(["domios", "tab", "list", "--workspace", workspace]))
        .collect();
    let agent_spawn: Vec<Value> = workspaces
        .iter()
        .filter(|_| permission.allowed && permission.allow_tab_requests)
        .map(|workspace| {
            json!([
                "domios",
                "agent",
                "spawn",
                "<provider>",
                "--workspace",
                workspace,
                "--description",
                "<summary>",
                "--task-file",
                "-"
            ])
        })
        .collect();
    let mut grants = serde_json::to_value(permission).expect("Neighbor permission is serializable");
    grants
        .as_object_mut()
        .expect("Neighbor permission is an object")
        .remove("scope");
    grants["allow_memory"] = json!(memory);
    grants["allow_files"] = json!(files);
    grants["allow_tab_requests"] = json!(permission.allowed && permission.allow_tab_requests);
    grants["commands"] = json!({
        "memory_neighbor_save":permission.memory_write_areas.iter().filter(|_|permission.allowed).map(|area|json!(["domios","mem","neighbor-save","--kind","fact","--title","<title>","--from-pad","<body.md>","--area",area])).collect::<Vec<_>>(),
        "memory_search":memory_search,
        "memory_get":(memory && areas.is_none_or(|areas|!areas.is_empty())).then(||json!(["domios","mem","get","<item_id>"])),
        "memory_wake_up":(memory && areas.is_none()).then(||json!(["domios","mem","wake-up"])),
        "file_reads":if files { file_read_commands(friendship) } else { vec![] },
        "tab_list":tab_list,
        "agent_spawn":agent_spawn,
        "tab_send":(permission.allowed && !workspaces.is_empty()).then(||json!(["domios","send","--tab","<allowed_agent_tab>","--from-pad","<payload.md>","--require-agent"])),
        "machine_read":if permission.allowed && permission.machine_access != codeterm_neighbors::permissions::MachineAccess::None {
            Some(if cfg!(windows) {json!(["Get-Content","-LiteralPath","<permitted_path>"])} else {json!(["cat","<permitted_path>"])})
        } else {None},
    });
    grants
}

fn file_read_commands(friendship: &str) -> Vec<String> {
    [json!({"operation":"list","friendship_id":friendship}),
     json!({"operation":"open","friendship_id":friendship,"file_id":"<saved_file_id>","launch":false})]
        .into_iter().map(|body| {
            if cfg!(windows) {
                format!("Invoke-RestMethod -Method Post -Uri ($env:CODETERM_API_URL + '/api/account/neighbors/files') -Headers @{{Authorization=('Bearer ' + $env:CODETERM_API_TOKEN)}} -ContentType 'application/json' -Body '{body}'")
            } else {
                format!("curl --fail-with-body -sS -X POST \"$CODETERM_API_URL/api/account/neighbors/files\" -H \"Authorization: Bearer $CODETERM_API_TOKEN\" -H 'Content-Type: application/json' --data '{body}'")
            }
        }).collect()
}
