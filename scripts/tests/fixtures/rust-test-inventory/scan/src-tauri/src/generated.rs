macro_rules! make_test {
    ($name:ident) => {
        #[test]
        fn $name() {}
    };
}

make_test!(generated_case);
