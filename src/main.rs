use anyhow::Result;
use enum_map::*;
use std::collections::HashMap;
use std::io::Read;
use tree_sitter::*;

#[derive(Debug, Clone)]
enum Input {
    StdIn,
    File(String),
}

#[derive(Clone, Debug)]
struct Opts {
    input: Input,
    debug: bool,
}

fn main() -> Result<()> {
    let opts = {
        use bpaf::*;
        let file = long("file").argument("FILE").map(Input::File);
        let stdin = long("stdin").req_flag(Input::StdIn);
        let debug = long("debug").short('d').switch();
        let input: Parser<Input> = stdin.or_else(file);
        // let input: Parser<Input> = stdin.or_else(file).fallback(Input::StdIn);
        let parser = construct!(Opts { input, debug });
        Info::default()
            .descr("Parse a C header file for signatures")
            .for_parser(parser)
            .run()
    };

    let mut parser = Parser::new();
    parser
        .set_language(tree_sitter_c::language())
        .expect("Error loading C grammar");
    let mut code = String::new();
    match opts.input {
        Input::StdIn => {
            std::io::stdin().read_to_string(&mut code)?;
        }
        Input::File(s) => {
            code = std::fs::read_to_string(&s)?;
        }
    };

    #[derive(Enum, Copy, Clone, PartialEq, Debug)]
    #[allow(non_camel_case_types)]
    enum Capture {
        fdecl,
        def,
        // error,
        // typedecl,
        // init,
        // vdecl,
        // msig,
        // mlambda,
        body,
        scs,
        name,
    }

    let query = Query::new(tree_sitter_c::language(), "
  ((function_definition . (storage_class_specifier)? @scs declarator: (function_declarator (identifier) @name) body: (_)? @body) @def)
  ((function_definition . (storage_class_specifier)? @scs declarator: (pointer_declarator declarator: (function_declarator (identifier) @name)) body: (_)? @body) @def)
  ")?;
  // ((declaration declarator: (function_declarator)) @fdecl)
  // ((declaration declarator: (pointer_declarator declarator: (function_declarator))) @fdecl)
    // ((ERROR)+ @error)
    // ((comment)+ @comment)
    // (translation_unit (struct_specifier) @structdecl)
    // (translation_unit (enum_specifier) @typedecl)
    // ((type_definition) @typedecl)
    // (translation_unit (declaration (init_declarator) @init) @vdecl)
    // ((ERROR) @msig (compound_literal_expression) @mlambda)
    let capture_names = query.capture_names();
    let capture_strs = enum_map! {
        name => {
            let name: Capture = name;
            format!("{:?}", name)
        },
    };
    let index_lookup = enum_map! {
        k => {
            let name = capture_strs[k].as_str();
            query
                .capture_index_for_name(name)
                .unwrap_or(1000000)
                // .expect(name)
        }
    };
    let tree = parser.parse(&code, None).expect("Failed to parse");
    let mut cursor = QueryCursor::new();
    let code_bytes = code.as_bytes();
    use Capture as C;
    let to_capture = enum_map! {
        // fdecl = 1;
        C::def => true, C::body => true, C::name => true, C::scs => true,
        // C::vdecl => true, C::init => true,
        // C::ident => true,
        // C::mlambda => true, C::msig => true,
        _ => false,
    };
    let tocapset: HashMap<u32, Capture> = to_capture
        .iter()
        .filter_map(|(k, v)| if *v { Some((index_lookup[k], k)) } else { None })
        .collect();
    let node_str = |n: Node| unsafe { std::str::from_utf8_unchecked(&code_bytes[n.byte_range()]) };
    for m in cursor.matches(&query, tree.root_node(), |n: Node| {
        std::iter::once(&code_bytes[n.byte_range()])
    }) {
        let mut captures: EnumMap<Capture, Option<_>> = Default::default();
        // m.pattern_index;
        // println!("{:?}", m);
        for cap in m.captures {
            // cap.node;
            // cap.index;
            if cap.index == index_lookup[C::fdecl] {
                println!("{}", node_str(cap.node));
            } else if let Some(x) = tocapset.get(&cap.index) {
                captures[*x] = Some(cap.node);
            } else if opts.debug {
                println!(
                    "{}: {:?}",
                    capture_names[cap.index as usize],
                    node_str(cap.node)
                );
            }
        }
        'skip_def: loop {
            if let Some(def) = captures[C::def] {
                if let Some(scs) = captures[C::scs] {
                    if node_str(scs).find("static").is_some() {
                        break 'skip_def;
                    }
                }
                let def = unsafe {
                    std::str::from_utf8_unchecked(
                        &code_bytes[def.start_byte()..captures[C::body].unwrap().start_byte()],
                    )
                };
                println!("{};", def.trim());
            }
            break 'skip_def;
        }
    }
    Ok(())
}

