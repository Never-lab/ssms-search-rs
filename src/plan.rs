//! Parse SQL Server ShowPlan XML into operator rows (SSMS Execution Plan pane).

#[derive(Debug, Clone)]
pub struct PlanOperator {
    pub physical_op: String,
    pub logical_op: String,
    pub estimate_rows: String,
    pub estimate_cost: String,
    pub node_id: String,
}

/// Extract RelOp highlights from ShowPlanXML without a full XML DOM.
pub fn parse_showplan_operators(xml: &str) -> Vec<PlanOperator> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(idx) = rest.find("<RelOp") {
        let slice = &rest[idx..];
        let end = slice.find('>').unwrap_or(slice.len().min(800));
        let tag = &slice[..end];
        out.push(PlanOperator {
            physical_op: attr(tag, "PhysicalOp").unwrap_or_else(|| "?".into()),
            logical_op: attr(tag, "LogicalOp").unwrap_or_else(|| "?".into()),
            estimate_rows: attr(tag, "EstimateRows").unwrap_or_default(),
            estimate_cost: attr(tag, "EstimatedTotalSubtreeCost")
                .or_else(|| attr(tag, "EstimateTotalSubtreeCost"))
                .unwrap_or_default(),
            node_id: attr(tag, "NodeId").unwrap_or_default(),
        });
        rest = &slice[end.saturating_add(1)..];
        if out.len() >= 500 {
            break;
        }
    }
    out
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let start = tag.find(&key)? + key.len();
    let tail = &tag[start..];
    let end = tail.find('"')?;
    Some(tail[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_relop() {
        let xml = r#"<ShowPlanXML><RelOp NodeId="1" PhysicalOp="Clustered Index Scan" LogicalOp="Clustered Index Scan" EstimateRows="100" EstimatedTotalSubtreeCost="0.01"></RelOp></ShowPlanXML>"#;
        let ops = parse_showplan_operators(xml);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].physical_op, "Clustered Index Scan");
        assert_eq!(ops[0].estimate_rows, "100");
    }
}
