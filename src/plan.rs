//! Parse SQL Server ShowPlan XML into operator tree (SSMS Execution Plan pane).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanOperator {
    pub physical_op: String,
    pub logical_op: String,
    pub estimate_rows: String,
    pub estimate_cost: String,
    pub node_id: String,
    /// Index of parent RelOp in the same Vec (`None` = root).
    pub parent: Option<usize>,
}

impl PlanOperator {
    pub fn depth(&self, ops: &[PlanOperator]) -> usize {
        let mut d = 0usize;
        let mut cur = self.parent;
        let mut guard = 0usize;
        while let Some(p) = cur {
            d += 1;
            cur = ops.get(p).and_then(|o| o.parent);
            guard += 1;
            if guard > ops.len() {
                break;
            }
        }
        d
    }
}

/// Extract RelOp nodes preserving nesting via `parent` indices.
pub fn parse_showplan_operators(xml: &str) -> Vec<PlanOperator> {
    let mut out = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut i = 0usize;

    while i < xml.len() {
        let rest = &xml[i..];
        let open_at = rest.find("<RelOp");
        let close_at = rest.find("</RelOp");

        let next_open = open_at.map(|o| (o, true));
        let next_close = close_at.map(|c| (c, false));
        let (rel, is_open) = match (next_open, next_close) {
            (Some((o, _)), Some((c, _))) if o <= c => (o, true),
            (Some((o, _)), None) => (o, true),
            (None, Some((c, _))) => (c, false),
            (Some((_, _)), Some((c, _))) => (c, false),
            (None, None) => break,
        };

        if is_open {
            let slice = &rest[rel..];
            let end = slice.find('>').unwrap_or(slice.len().min(800));
            let tag = &slice[..end];
            let self_closing =
                tag.ends_with('/') || slice.get(..end + 1).is_some_and(|s| s.ends_with("/>"));
            let parent = stack.last().copied();
            let idx = out.len();
            out.push(PlanOperator {
                physical_op: attr(tag, "PhysicalOp").unwrap_or_else(|| "?".into()),
                logical_op: attr(tag, "LogicalOp").unwrap_or_else(|| "?".into()),
                estimate_rows: attr(tag, "EstimateRows").unwrap_or_default(),
                estimate_cost: attr(tag, "EstimatedTotalSubtreeCost")
                    .or_else(|| attr(tag, "EstimateTotalSubtreeCost"))
                    .unwrap_or_default(),
                node_id: attr(tag, "NodeId").unwrap_or_default(),
                parent,
            });
            if !self_closing {
                stack.push(idx);
            }
            i += rel + end + 1;
            if out.len() >= 500 {
                break;
            }
        } else {
            let _ = stack.pop();
            // skip "</RelOp" ... ">"
            let slice = &rest[rel..];
            let end = slice.find('>').unwrap_or(6);
            i += rel + end + 1;
        }
    }
    out
}

/// Children indices for each operator (stable document order).
pub fn plan_children(ops: &[PlanOperator]) -> Vec<Vec<usize>> {
    let mut kids = vec![Vec::new(); ops.len()];
    for (i, op) in ops.iter().enumerate() {
        if let Some(p) = op.parent {
            if p < kids.len() {
                kids[p].push(i);
            }
        }
    }
    kids
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
        assert!(ops[0].parent.is_none());
    }

    #[test]
    fn parses_nested_relops_with_parent_links() {
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
        assert!(ops[0].parent.is_none());
        assert_eq!(ops[1].parent, Some(0));
        assert_eq!(ops[2].parent, Some(0));
        assert_eq!(ops[2].estimate_cost, "0.003");
        let kids = plan_children(&ops);
        assert_eq!(kids[0], vec![1, 2]);
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

    #[test]
    fn depth_follows_parent_chain() {
        let xml = r#"<RelOp NodeId="0" PhysicalOp="A" LogicalOp="A" EstimateRows="1" EstimatedTotalSubtreeCost="1"><RelOp NodeId="1" PhysicalOp="B" LogicalOp="B" EstimateRows="1" EstimatedTotalSubtreeCost="0.1"><RelOp NodeId="2" PhysicalOp="C" LogicalOp="C" EstimateRows="1" EstimatedTotalSubtreeCost="0.01"/></RelOp></RelOp>"#;
        let ops = parse_showplan_operators(xml);
        assert_eq!(ops.len(), 3);
        assert_eq!(ops[0].depth(&ops), 0);
        assert_eq!(ops[1].depth(&ops), 1);
        assert_eq!(ops[2].depth(&ops), 2);
    }
}
