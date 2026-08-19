use std::{
    fs,
    path::Path,
};

use proc_macro2::Span;
use serde_json::json;
use syn::{
    ImplItem,
    Item,
    spanned::Spanned,
    visit::{
        self,
        Visit,
    },
};

use crate::{
    error::AuditError,
    models::{
        AuditFinding,
        IndexedFile,
        Severity,
        StaticMetricsSummary,
        TrialStatus,
    },
};

pub struct StaticMetricsResult {
    pub metric: StaticMetricsSummary,
    pub findings: Vec<AuditFinding>,
}

pub fn evaluate(
    repo_root: &Path,
    files: &[IndexedFile],
    threshold: usize,
) -> Result<StaticMetricsResult, AuditError> {
    let mut function_metrics = Vec::new();
    let mut parse_failures = 0usize;

    for file in files.iter().filter(|file| file.language == "rust") {
        let file_path = repo_root.join(&file.path);
        let content = fs::read_to_string(&file_path)?;
        let syntax = match syn::parse_file(&content) {
            Ok(syntax) => syntax,
            Err(_) => {
                parse_failures += 1;
                continue;
            },
        };

        collect_function_metrics(
            &file.path,
            &syntax.items,
            None,
            &mut function_metrics,
        );
    }

    let high_complexity_functions = function_metrics
        .iter()
        .filter(|metric| metric.complexity > threshold)
        .count();
    let average_cyclomatic_complexity = if function_metrics.is_empty() {
        None
    } else {
        Some(
            function_metrics
                .iter()
                .map(|metric| metric.complexity as f64)
                .sum::<f64>()
                / function_metrics.len() as f64,
        )
    };
    let max_cyclomatic_complexity = function_metrics
        .iter()
        .map(|metric| metric.complexity)
        .max();

    let findings = function_metrics
        .iter()
        .filter(|metric| metric.complexity > threshold)
        .map(|metric| AuditFinding {
            id: format!("static_complexity:{}:{}", metric.path, metric.name),
            category: "static_complexity".to_string(),
            severity: if metric.complexity >= threshold * 2 {
                Severity::High
            } else {
                Severity::Medium
            },
            summary: format!(
                "{} in {} has cyclomatic complexity {}, exceeding the limit of {}.",
                metric.name, metric.path, metric.complexity, threshold
            ),
            path: Some(metric.path.clone()),
            line: Some(metric.start_line),
            metric_name: "cyclomatic_complexity".to_string(),
            metric_value: json!(metric.complexity),
            threshold: Some(json!(threshold)),
            instructions: vec![
                format!(
                    "Refactor {} in {} into smaller helpers so each branch can be tested independently.",
                    metric.name, metric.path
                ),
                "Replace nested conditionals with smaller pure functions or table-driven dispatch where possible.".to_string(),
            ],
            evidence: json!({
                "path": metric.path,
                "function": metric.name,
                "start_line": metric.start_line,
                "end_line": metric.end_line,
                "cyclomatic_complexity": metric.complexity,
            }),
        })
        .collect();

    Ok(StaticMetricsResult {
        metric: StaticMetricsSummary {
            status: TrialStatus::Collected,
            threshold,
            functions_analyzed: function_metrics.len(),
            parse_failures,
            high_complexity_functions,
            average_cyclomatic_complexity,
            max_cyclomatic_complexity,
            details: None,
        },
        findings,
    })
}

#[derive(Debug)]
struct FunctionMetric {
    path: String,
    name: String,
    start_line: usize,
    end_line: usize,
    complexity: usize,
}

fn collect_function_metrics(
    file_path: &str,
    items: &[Item],
    module_path: Option<&str>,
    output: &mut Vec<FunctionMetric>,
) {
    for item in items {
        match item {
            Item::Fn(item_fn) => {
                let function_name =
                    qualify_name(module_path, &item_fn.sig.ident.to_string());
                output.push(FunctionMetric {
                    path: file_path.to_string(),
                    name: function_name,
                    start_line: start_line(item_fn.span()),
                    end_line: end_line(item_fn.span()),
                    complexity: complexity_for_block(&item_fn.block),
                });
            },
            Item::Impl(item_impl) =>
                for impl_item in &item_impl.items {
                    if let ImplItem::Fn(method) = impl_item {
                        let method_name = qualify_name(
                            module_path,
                            &method.sig.ident.to_string(),
                        );
                        output.push(FunctionMetric {
                            path: file_path.to_string(),
                            name: method_name,
                            start_line: start_line(method.span()),
                            end_line: end_line(method.span()),
                            complexity: complexity_for_block(&method.block),
                        });
                    }
                },
            Item::Mod(item_mod) => {
                if let Some((_, nested_items)) = &item_mod.content {
                    let next_module_path =
                        qualify_name(module_path, &item_mod.ident.to_string());
                    collect_function_metrics(
                        file_path,
                        nested_items,
                        Some(&next_module_path),
                        output,
                    );
                }
            },
            _ => {},
        }
    }
}

fn qualify_name(
    module_path: Option<&str>,
    name: &str,
) -> String {
    match module_path {
        Some(module_path) if !module_path.is_empty() =>
            format!("{module_path}::{name}"),
        _ => name.to_string(),
    }
}

fn start_line(span: Span) -> usize {
    span.start().line
}

fn end_line(span: Span) -> usize {
    span.end().line
}

fn complexity_for_block(block: &syn::Block) -> usize {
    let mut visitor = ComplexityVisitor { complexity: 1 };
    visitor.visit_block(block);
    visitor.complexity
}

struct ComplexityVisitor {
    complexity: usize,
}

impl<'ast> Visit<'ast> for ComplexityVisitor {
    fn visit_expr_if(
        &mut self,
        node: &'ast syn::ExprIf,
    ) {
        self.complexity += 1;
        visit::visit_expr_if(self, node);
    }

    fn visit_expr_for_loop(
        &mut self,
        node: &'ast syn::ExprForLoop,
    ) {
        self.complexity += 1;
        visit::visit_expr_for_loop(self, node);
    }

    fn visit_expr_while(
        &mut self,
        node: &'ast syn::ExprWhile,
    ) {
        self.complexity += 1;
        visit::visit_expr_while(self, node);
    }

    fn visit_expr_loop(
        &mut self,
        node: &'ast syn::ExprLoop,
    ) {
        self.complexity += 1;
        visit::visit_expr_loop(self, node);
    }

    fn visit_expr_match(
        &mut self,
        node: &'ast syn::ExprMatch,
    ) {
        self.complexity += node.arms.len();
        visit::visit_expr_match(self, node);
    }

    fn visit_expr_binary(
        &mut self,
        node: &'ast syn::ExprBinary,
    ) {
        if matches!(node.op, syn::BinOp::And(_) | syn::BinOp::Or(_)) {
            self.complexity += 1;
        }
        visit::visit_expr_binary(self, node);
    }
}
