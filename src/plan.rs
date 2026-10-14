//! Parse SQL Server ShowPlan XML into operator rows (SSMS Execution Plan pane).

#[derive(Debug, Clone, PartialEq, Eq)]
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
    fn parses_single_relop() {
        let xml = r#"<ShowPlanXML><RelOp NodeId="1" PhysicalOp="Clustered Index Scan" LogicalOp="Clustered Index Scan" EstimateRows="100" EstimatedTotalSubtreeCost="0.01"></RelOp></ShowPlanXML>"#;
        let ops = parse_showplan_operators(xml);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].node_id, "1");
        assert_eq!(ops[0].physical_op, "Clustered Index Scan");
        assert_eq!(ops[0].logical_op, "Clustered Index Scan");
        assert_eq!(ops[0].estimate_rows, "100");
        assert_eq!(ops[0].estimate_cost, "0.01");
    }

    #[test]
    fn parses_multiple_relops_in_document_order() {
        let xml = r#"
        <ShowPlanXML>
          <RelOp NodeId="0" PhysicalOp="Nested Loops" LogicalOp="Inner Join" EstimateRows="10" EstimatedTotalSubtreeCost="1.5">
            <RelOp NodeId="1" PhysicalOp="Index Seek" LogicalOp="Index Seek" EstimateRows="1" EstimatedTotalSubtreeCost="0.003"/>
            <RelOp NodeId="2" PhysicalOp="Key Lookup" LogicalOp="Key Lookup" EstimateRows="1" EstimateTotalSubtreeCost="0.003"/>
          </RelOp>
        </ShowPlanXML>"#;
        let ops = parse_showplan_operators(xml);
        assert_eq!(ops.len(), 3);
        assert_eq!(ops[0].physical_op, "Nested Loops");
        assert_eq!(ops[1].physical_op, "Index Seek");
        assert_eq!(ops[2].physical_op, "Key Lookup");
        assert_eq!(ops[2].estimate_cost, "0.003"); // alternate attribute name
    }

    #[test]
    fn empty_and_garbage_yield_empty() {
        assert!(parse_showplan_operators("").is_empty());
        assert!(parse_showplan_operators("<root/>").is_empty());
        assert!(parse_showplan_operators("RelOp PhysicalOp=nope").is_empty());
    }

    #[test]
    fn missing_attrs_use_defaults() {
        let ops = parse_showplan_operators(r#"<RelOp NodeId="9"></RelOp>"#);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].node_id, "9");
        assert_eq!(ops[0].physical_op, "?");
        assert_eq!(ops[0].logical_op, "?");
        assert!(ops[0].estimate_rows.is_empty());
        assert!(ops[0].estimate_cost.is_empty());
    }

    #[test]
    fn caps_at_500_operators() {
        let mut xml = String::from("<ShowPlanXML>");
        for i in 0..600 {
            xml.push_str(&format!(
                r#"<RelOp NodeId="{i}" PhysicalOp="Compute Scalar" LogicalOp="Compute Scalar" EstimateRows="1" EstimatedTotalSubtreeCost="0"/>"#
            ));
        }
        xml.push_str("</ShowPlanXML>");
        let ops = parse_showplan_operators(&xml);
        assert_eq!(ops.len(), 500);
        assert_eq!(ops[0].node_id, "0");
        assert_eq!(ops[499].node_id, "499");
    }

    #[test]
    fn attr_stops_at_first_quote() {
        let tag = r#"PhysicalOp="Hash Match" LogicalOp="Aggregate""#;
        assert_eq!(attr(tag, "PhysicalOp").as_deref(), Some("Hash Match"));
        assert_eq!(attr(tag, "LogicalOp").as_deref(), Some("Aggregate"));
        assert!(attr(tag, "Missing").is_none());
    }
}
