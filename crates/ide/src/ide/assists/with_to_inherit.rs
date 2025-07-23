use super::{AssistKind, AssistsCtx};
use crate::def::{AstPtr, ResolveResult};
use crate::TextEdit;
use itertools::Itertools;
use syntax::ast::AstNode;
use syntax::{ast, TextRange};

pub(super) fn convert_to_inherit(ctx: &mut AssistsCtx<'_>) -> Option<()> {
    let file_id = ctx.frange.file_id;
    let name_res = ctx.db.name_resolution(file_id);
    let source_map = ctx.db.source_map(file_id);

    let with = ctx.covering_node::<ast::With>()?;
    let with_pos = TextRange::new(
        with.with_token()?.text_range().start(),
        with.semicolon_token()?.text_range().end(),
    );
    let with_env = with.environment()?;
    let with_ptr = AstPtr::new(with.syntax());

    let usages = name_res
        .iter()
        .filter_map(|(usage, res)| match res {
            ResolveResult::WithExprs(envs) => {
                let closest_env = envs.first().expect("must have one environment");
                let env_ptr = source_map.node_for_expr(*closest_env)?;
                if env_ptr == with_ptr {
                    Some(
                        source_map
                            .node_for_expr(usage)?
                            .to_node(ctx.ast.syntax())
                            .text()
                            .to_string(),
                    )
                } else {
                    None
                }
            }
            _ => None,
        })
        .sorted()
        .dedup()
        .collect::<Vec<_>>();
    // There are no usages that depend on this with env
    if usages.is_empty() {
        return None;
    }

    let mut rewrites: Vec<TextEdit> = vec![];
    rewrites.push(TextEdit {
        delete: with_pos,
        insert: format!(
            "let inherit ({}) {}; in",
            with_env.syntax().text(),
            usages.join(" "),
        )
        .into(),
    });
    ctx.add(
        "convert_with_to_inherit",
        "Convert with to inherit",
        AssistKind::RefactorRewrite,
        rewrites,
    );

    Some(())
}

#[cfg(test)]
mod tests {
    use expect_test::expect;
    define_check_assist!(super::convert_to_inherit);

    #[test]
    fn single_environment() {
        check(
            "$0with lib; x y z",
            expect!["let inherit (lib) x y z; in x y z"],
        );
    }

    #[test]
    fn multiple_environments() {
        check_no("let bar = 1; in $0with foo; with bar; x y z");
        check_no("{bar, ...}: $0with foo; with bar; x y z");
        check(
            "let bar = 1; in with foo; $0with bar; x y z",
            expect!["let bar = 1; in with foo; let inherit (bar) x y z; in x y z"],
        );
    }

    #[test]
    fn nixos_module() {
        check_no(
            "{lib, config, pkgs, ...}:
$0with lib;
with lib.types;
{
    options.example = mkOption {
        type = types.attrsOf types.str;
        default = { };
    };
    environment.systemPackages = with pkgs; [ hello nixfmt nil ];
}",
        );

        check(
            "{lib, config, pkgs, ...}:
$0with lib;
{
    options.example = mkOption {
        type = types.attrsOf types.str;
        default = { };
    };
    environment.systemPackages = with pkgs; [ hello nixfmt nil ];
}",
            expect![[r#"
                {lib, config, pkgs, ...}:
                let inherit (lib) mkOption types; in
                {
                    options.example = mkOption {
                        type = types.attrsOf types.str;
                        default = { };
                    };
                    environment.systemPackages = with pkgs; [ hello nixfmt nil ];
                }
            "#]],
        );

        // a with expression lower down in the ast
        check(
            "{lib, config, pkgs, ...}:
with lib;
{
    options.example = mkOption {
        type = types.attrsOf types.str;
        default = { };
    };
    environment.systemPackages = $0with pkgs; [ hello nixfmt nil ];
}",
            expect![[r#"
                {lib, config, pkgs, ...}:
                with lib;
                {
                    options.example = mkOption {
                        type = types.attrsOf types.str;
                        default = { };
                    };
                    environment.systemPackages = let inherit (pkgs) hello nil nixfmt; in [ hello nixfmt nil ];
                }
            "#]],
        );

        // foo is bound on the outside
        check(
            r#"let foo = "foo";
in pkgs: $0with pkgs; [ foo bar ]"#,
            expect![[r#"
                let foo = "foo";
                in pkgs: let inherit (pkgs) bar; in [ foo bar ]
            "#]],
        );
    }
}
