fn main() {
    if plexmaton_file_tools::run_search_driver(std::env::args_os().skip(1)).is_err() {
        std::process::exit(125);
    }
}
